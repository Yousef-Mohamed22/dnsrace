#!/usr/bin/env bash
# dnsrace - race DNS servers from YOUR connection and find the fastest one for your location.
#
# Usage:  bash dnsrace.sh [hostname]
#   hostname (optional): a site that is slow for you, e.g. github.com
#                        -> also shows which IPs the fastest servers hand out and how far away they are.
#
# Options (environment variables):
#   COUNTRIES="eg it"   also test the open resolvers public-dns.info lists for these 2-letter country codes
#   KNOWN=20            how many of the fastest servers from servers.txt get the full test
#   OTHER=10            how many of the fastest COUNTRIES resolvers get the full test
#   SERVERS_FILE=FILE   use a different server list (default: servers.txt next to this script)
#
# Needs: bash, dig, curl, awk, xargs   (dig: Arch "bind", Debian/Ubuntu "dnsutils", Fedora "bind-utils")
#
# How it works
#   1. Quick scan: one lookup against every candidate, 40 at a time; dead servers drop out.
#      Candidates = servers.txt + your router + your current DNS (+ the COUNTRIES lists).
#   2. Full test of the fastest ones (your current DNS is always included):
#        cached   = 30 lookups of popular sites  (mostly shows how far away the server is)
#        uncached = 5 lookups of random names    (shows how good the server's own upstream is)
#      Ranked by the average of the two, lower is better. Nothing on your system is changed.

DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
COUNTRIES="${COUNTRIES-}"
KNOWN="${KNOWN:-20}"
OTHER="${OTHER:-10}"
SERVERS_FILE="${SERVERS_FILE:-$DIR/servers.txt}"
DOMAINS="google.com youtube.com facebook.com github.com wikipedia.org amazon.com steampowered.com archlinux.org reddit.com microsoft.com"
HOST="${1:-}"

command -v dig >/dev/null || { echo "dig not found. Arch: sudo pacman -S bind | Debian/Ubuntu: sudo apt install dnsutils | Fedora: sudo dnf install bind-utils"; exit 1; }
command -v curl >/dev/null || { echo "curl not found. Please install curl."; exit 1; }
[ -r "$SERVERS_FILE" ] || { echo "Server list not found: $SERVERS_FILE (keep servers.txt next to dnsrace.sh, or set SERVERS_FILE=...)"; exit 1; }
RESULTS=$(mktemp); trap 'rm -f "$RESULTS"' EXIT

echo "== Cloudflare data center you reach (first 3 letters = nearest city's airport code, e.g. cai = Cairo, mrs = Marseille, fra = Frankfurt) =="
dig +short +time=2 +tries=1 CHAOS TXT id.server @1.1.1.1
echo

# ---- candidates: servers.txt, your router, your current DNS, then the public-dns.info lists ----
NAMED=$(tr -d '\r' < "$SERVERS_FILE" | grep -vE '^[[:space:]]*(#|$)')
GW=$(ip route show default 2>/dev/null | awk '{print $3; exit}')
CURRENT=$(
  { resolvectl dns 2>/dev/null
    nmcli -g IP4.DNS dev show 2>/dev/null
    grep -h '^nameserver' /etc/resolv.conf 2>/dev/null
  } | grep -oE '[0-9]+(\.[0-9]+){3}' | grep -v '^127\.' | sort -u
)
[ -n "$CURRENT" ] && { echo "Your current DNS: ${CURRENT//$'\n'/ }"; echo; }

POOL=$(
  {
    printf '%s\n' "$NAMED"
    [ -n "$GW" ] && echo "$GW Router/ISP-default"
    [ -n "$CURRENT" ] && awk '{print $1 " current"}' <<<"$CURRENT"
    for cc in $COUNTRIES; do
      cc=$(tr '[:upper:]' '[:lower:]' <<<"$cc")
      [[ "$cc" =~ ^[a-z]{2}$ ]] || { echo "note: skipping '$cc' (use 2-letter country codes like eg, it, fr)" >&2; continue; }
      list=$(curl -fsS --max-time 20 "https://public-dns.info/nameserver/$cc.txt" 2>/dev/null | tr -d '\r' | grep -E '^[0-9]+(\.[0-9]+){3}$')
      if [ -z "$list" ]; then echo "note: could not download the '$cc' list from public-dns.info" >&2; else awk '{print $1 " other"}' <<<"$list"; fi
    done
  } | awk 'NF >= 2 && !seen[$1]++'
)
[ -z "$POOL" ] && { echo "No servers to test: is $SERVERS_FILE empty?"; exit 1; }

# ---- stage 1: quick scan, one lookup per candidate, 40 in parallel ----
probe() {
  local out ms
  out=$(dig @"$1" google.com +tries=2 +time=1 2>/dev/null) || return 0
  grep -q 'status: NOERROR' <<<"$out" || return 0
  ms=$(awk '/Query time/ {print $4}' <<<"$out")
  echo "${ms:-9999} $1"
}
export -f probe
EXTRA=$(grep -c ' other$' <<<"$POOL"); EXTRA_NOTE=""
[ "$EXTRA" -gt 0 ] && EXTRA_NOTE=" ($EXTRA from public-dns.info)"
echo "Stage 1: quick scan of $(wc -l <<<"$POOL" | tr -d ' ') candidates$EXTRA_NOTE..."
# shellcheck disable=SC2016  # the single quotes are intentional: the inner bash expands $1
SCAN=$(awk '{print $1}' <<<"$POOL" | xargs -n1 -P40 bash -c 'probe "$1"' _ | sort -n)
[ -z "$SCAN" ] && { echo "No server answered. Is your connection up (and IPv4 enabled)?"; exit 1; }
echo "$(wc -l <<<"$SCAN" | tr -d ' ') answered."

SHORT=$(
  {
    awk -v tk="$KNOWN" -v to="$OTHER" '
      NR == FNR { name[$1] = $2; next }
      { ip = $2
        if (name[ip] == "other") { if (++o <= to) print ip }
        else if (++k <= tk) print ip }' <(echo "$POOL") <(echo "$SCAN")
    for m in $CURRENT; do
      if grep -qwF "$m" <<<"$SCAN"; then echo "$m"; else echo "note: your current DNS $m did not answer the quick scan" >&2; fi
    done
  } | awk '!seen[$0]++'
)

# ---- stage 2: full test of the fastest ones ----
lookup() {  # $1=server $2=name -> sets ms and st; returns 1 if there was no reply
  local out
  out=$(dig @"$1" "$2" +tries=1 +time=2 2>/dev/null) || return 1
  ms=$(awk '/Query time/ {print $4}' <<<"$out")
  st=$(sed -n 's/.*status: \([A-Z]*\).*/\1/p' <<<"$out" | head -n1)
  [ -n "$ms" ]
}

echo
echo "Stage 2: full test of $(wc -w <<<"$SHORT" | tr -d ' ') servers (usually 1-3 minutes)..."
for s in $SHORT; do
  name=$(awk -v ip="$s" '$1 == ip {print $2; exit}' <<<"$POOL")
  ct=0 cn=0 cf=0 ut=0 un=0 hij=0 bad=0
  for d in $DOMAINS; do
    for _ in 1 2 3; do
      if lookup "$s" "$d" && [ "$st" = NOERROR ]; then
        ct=$((ct+ms)); cn=$((cn+1))
      else
        cf=$((cf+1))
        if [ "$cf" -ge 8 ]; then bad=1; break 2; fi   # too many failures: stop wasting time on this one
      fi
    done
  done
  if [ "$bad" = 0 ]; then
    for _ in 1 2 3 4 5; do
      if lookup "$s" "dnsbench-$RANDOM-$RANDOM-$RANDOM.com"; then
        case "$st" in
          NXDOMAIN) ;;
          NOERROR) hij=$((hij+1)) ;;
          *) cf=$((cf+1)); continue ;;
        esac
        ut=$((ut+ms)); un=$((un+1))
      else
        cf=$((cf+1))
      fi
    done
  fi
  c=$((cn ? ct/cn : 9999)); u=$((un ? ut/un : 9999))
  note=""
  [ "$hij" -gt 0 ] && note="HIJACKS-NXDOMAIN"
  [ "$bad" = 1 ] && { c=9999; u=9999; note="UNRELIABLE"; }
  score=$(( (c+u)/2 ))
  [ "$hij" -gt 0 ] && score=$((score+5000))   # flagged servers sort to the bottom
  echo "$score|$s|$name|$c|$u|$cf|$note" >> "$RESULTS"
done

echo
echo "== Results: average of cached + uncached, lower is better =="
sort -t'|' -k1,1n "$RESULTS" | awk -F'|' -v cur="$(tr '\n' ' ' <<<"$CURRENT")" '
  BEGIN { n = split(cur, a, " "); for (i = 1; i <= n; i++) c[a[i]] = 1 }
  { printf "%2d. %-16s %-20s cached %4d ms  uncached %4d ms  timeouts %s%s%s\n", NR, $2, $3, $4, $5, $6, ($7 != "" ? "  " $7 : ""), (($2 in c) ? "  <== current" : "")
    if ($2 in c) mine = mine (mine ? ", " : "") "#" NR " (" $2 ")"
    if ($7 == "" && $6 <= 3) {
      if (!best) best = $2 " (" $3 ")"
      if (!known && $3 != "other" && $3 != "current" && $3 !~ /^Router/) known = $2 " (" $3 ")"
    } }
  END { print ""
        print "Fastest overall:        " best
        print "Fastest known provider: " known
        if (mine) print "Your current DNS:       " mine " of " NR " tested" }'

# ---- optional: what does the site you care about get steered to? ----
if [ -n "$HOST" ]; then
  TOPIPS=$(sort -t'|' -k1,1n "$RESULTS" | head -n 10 | cut -d'|' -f2)
  NTOP=$(wc -w <<<"$TOPIPS" | tr -d ' ')
  echo
  echo "== $HOST: IPs handed out by the $NTOP fastest servers, and TCP connect time to each =="
  for s in $TOPIPS; do dig +short +tries=1 +time=2 @"$s" "$HOST"; done \
    | grep -E '^[0-9]+(\.[0-9]+){3}$' | sort | uniq -c | while read -r cnt ip; do
        ct=$(curl -so /dev/null --connect-timeout 5 --resolve "$HOST:443:$ip" -w '%{time_connect}s' "https://$HOST") || ct=FAILED
        printf '%-16s connect %-10s returned by %s of %s servers\n' "$ip" "$ct" "$cnt" "$NTOP"
      done
fi

echo
[ -n "$COUNTRIES" ] && echo "Servers named 'other' come from public-dns.info: open resolvers, often other people's routers."
echo "A DNS server sees every site you visit and can return fake answers, so only switch to one you trust."
echo "How to switch: see 'Use the winner' in the README."
