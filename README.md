# dnsrace

> Race DNS servers from **your own connection** and find the fastest one for **your location**.

The "best" DNS server depends on where you are and how your ISP routes to each provider, so a top-10 list from the internet can't tell you which one is best for you. `dnsrace` measures it: it tests about 85 well-known public resolvers (IPv4 and IPv6, and optionally every open resolver listed for your country) and ranks them by real lookup speed from your machine.

- **Fast.** A single Rust binary that speaks DNS itself: servers are tested in parallel and a full run usually takes well under 30 seconds.
- **Nothing to install.** No `dig`, no `curl`, no runtime. Download one file and run it.
- **Works everywhere.** Linux, macOS and Windows, IPv4 and IPv6.
- **Safe.** It only measures. It never changes your system's DNS settings.

**New in 0.5.0:** a graphical user interface (`--gui`), DNS-over-HTTPS (`--doh`), TCP DNS
(`--tcp`), multiple hostnames, enhanced statistics (mean/max/p99), file export
(`--export-json/--csv/--html`), and HTML reports. **New in 0.4.0:** a plain-language
recommendation and verdict, `--nearby` for local resolvers, `-q` for scripts, CSV output,
latency spread (min/p95/jitter), a DNSSEC check, `--repeat` for noisy connections, and
IPv4/IPv6 filters. See [CHANGELOG.md](CHANGELOG.md) for the full list.

## Install

**Download a build** from the [Releases page](https://github.com/Yousef-Mohamed22/dnsrace/releases): Linux (x86_64 and ARM64), macOS (Apple Silicon and Intel) and Windows (x86_64). Each platform has a terminal build and a `-gui` build; the GUI build also accepts every CLI flag. On Linux and macOS, make the downloaded file executable with `chmod +x dnsrace-*`, then run it. Each release has a `SHA256SUMS.txt` to check your download.

- macOS: the binaries are not signed, so the first run may be blocked. Remove the quarantine flag once with `xattr -d com.apple.quarantine ./dnsrace`.
- Windows: SmartScreen may warn about an unsigned program; choose "More info" then "Run anyway", or run it from PowerShell: `.\dnsrace.exe`.

**Or build it yourself** with [Rust](https://rustup.rs) 1.99 or newer:

```bash
cargo install --git https://github.com/Yousef-Mohamed22/dnsrace
# or, from a clone:
cargo build --release          # the binary is target/release/dnsrace
```

## Quick start

```bash
dnsrace                # race the built-in list (start here)
dnsrace --nearby       # also test the open resolvers near your country
dnsrace -v             # same, with latency spread and DNSSEC details
```

If you only remember one command, use `dnsrace --nearby`. It detects your country through Cloudflare and adds the public resolvers [public-dns.info](https://public-dns.info/) lists for it.

Reading the last lines is enough:

```
Recommended:            1.1.1.1 (Cloudflare) - 55.6 ms average
Fastest overall:        192.168.1.1 (Router/ISP-default) - 29.5 ms average
Your current DNS:       #16 (2.56.220.2) of 26 tested - 12.3 ms slower per lookup than the recommendation
DNSSEC validation:      8 of 12 healthy servers set the AD bit
Verdict:                switching to 1.1.1.1 would save about 12.3 ms per DNS lookup.
```

## Usage

```bash
dnsrace [OPTIONS] [HOSTNAME...]
```

| Option | What it does |
|---|---|
| `HOSTNAME` | a site that is slow for you (e.g. `github.com`): also show which IPs the fastest servers hand out and how far away they are |
| `-c, --countries <CC,...>` | also test the open resolvers [public-dns.info](https://public-dns.info/) lists for these 2-letter country codes (e.g. `eg,it,fr`) |
| `--nearby` | also test the open resolvers listed for your country (detected through Cloudflare; best effort) |
| `-k, --known <N>` | fully test the N fastest servers from the list (default 30) |
| `-o, --other <N>` | fully test the N fastest country resolvers (default 10) |
| `-s, --servers <FILE>` | use your own server list instead of the built-in one |
| `-a, --add <ADDR,...>` | also test these servers (IPv4 or IPv6, optionally with `:port`) |
| `-4, --ipv4` | only test IPv4 servers |
| `-6, --ipv6` | only test IPv6 servers |
| `-r, --repeat <N>` | repeat the full test N times and pool the measurements (1-5, default 1); slower but steadier |
| `-q, --quiet` | print only the recommended server address, nothing else |
| `--csv` | print the results as CSV on stdout (progress goes to stderr) |
| `--json` | print the results as JSON on stdout (progress goes to stderr) |
| `-v, --verbose` | add latency spread and DNSSEC details to the table |
| `--top <N>` | show only the N fastest rows in the table (default: all) |
| `-j, --jobs <N>` | how many servers to fully test at once (default 16) |
| `--timeout <MS>` | per-lookup timeout (default 1000; the quick scan uses at most 3000) |
| `--connect-port <PORT>` | TCP port for the connect-time check of `HOSTNAME` (default 443) |
| `--no-detect` | don't look up your router, your current DNS servers or your Cloudflare data center |
| `--no-color` | don't use colors |
| `--gui` | open the graphical interface (build with `--features gui`) |
| `--export-json <FILE>` | write the JSON results to this file |
| `--export-csv <FILE>` | write the CSV results to this file |
| `--export-html <FILE>` | write an HTML report to this file |
| `--tcp` | query DNS servers over TCP instead of UDP |
| `--doh` | also test Cloudflare and Google DNS-over-HTTPS providers |

`dnsrace --help` shows the same list. Your router and your current DNS servers are detected automatically. They, and servers added with `--add`, are always fully tested on top of the `--known`/`--other` picks and do not consume those slots; current servers are marked `<== current`. Exit status: 0 on success, 1 if no server could be tested (or, with `--quiet`, no healthy server to recommend), 2 for a usage error.

### Common runs

```bash
dnsrace --nearby                 # the everyday choice
dnsrace --repeat 3               # steadier ranking on a noisy connection
dnsrace --ipv4                   # skip IPv6 if your network's IPv6 is broken
dnsrace --countries eg -v        # Egypt's open resolvers, with details
dnsrace -q                       # just the winner: 1.1.1.1
dnsrace --csv > results.csv      # open in LibreOffice or Excel
dnsrace github.com               # is DNS steering github.com badly?
```

### Graphical interface

Build with the `gui` feature and launch with `--gui`:

```bash
cargo build --release --features gui
./target/release/dnsrace --gui
```

The GUI lets you set hostnames, timeout, concurrency, repeat count, address family, output
format, and toggles for TCP, DoH, system detection, and nearby resolvers. Click **Run
Benchmark** to see live progress, then view the results table and copy it to the clipboard.


## Reading the results

The main table ranks servers by the average of the cached and uncached medians, lower is better:

```
== Results: average of the cached and uncached medians, lower is better ==
 1. 192.168.1.1          Router/ISP-default  cached    5.0 ms  uncached   54.0 ms  timeouts 0
 2. 1.1.1.1              Cloudflare          cached   45.0 ms  uncached   66.0 ms  timeouts 0
 3. 2606:4700:4700::1111 Cloudflare          cached   47.0 ms  uncached   67.0 ms  timeouts 0
...
16. 2.56.220.2           Gcore               cached   54.0 ms  uncached   68.0 ms  timeouts 0  <== current
```

- **cached**: median of 30 lookups of popular sites, after an untimed warm-up pass so the names are already cached. Mostly measures how far away the server is.
- **uncached**: median of 5 lookups of random names that nobody has cached. Shows how good the server's own connection to the rest of the internet is. This is the fair number to compare against your router, which wins "cached" simply because it sits on your LAN.
- **timeouts**: lookups that got no answer. More than a few means an unreliable server.
- **Recommended**: the fastest healthy server from the built-in provider list. This is the one to switch to. If no listed provider is healthy, the fastest healthy server of any kind is used.
- **Fastest overall**: shown only when it differs from the recommendation (often your router).
- **Your current DNS**: where each current server ranked, and how many ms per lookup the best one is behind (or ahead of) the recommendation.
- **DNSSEC validation**: how many healthy servers set the AD (Authenticated Data) bit for a signed name. AD means the server (or its upstream) validates DNSSEC. It is not a guarantee of filtering or privacy.
- **Close call**: if the top two providers are within 1.5 ms, run again with `--repeat 3` before deciding.
- **Verdict**: a plain-language answer: switch, stay, or "the difference is too small to matter".
- `HIJACKS-NXDOMAIN`: the server returns a fake answer for names that don't exist (usually an ad or redirect page). Don't use it.
- `UNRELIABLE`: eight or more failed lookups in a round (scaled up by `--repeat`). These always rank last.

### Detail view

`dnsrace -v` adds a second table:

```
== Details: cached-lookup spread, and whether the server sets the DNSSEC AD bit ==
 1. 1.1.1.1          Cloudflare  min   40.2 ms  p95   51.8 ms  jitter    2.4 ms  DNSSEC yes
```

- **min**: the fastest cached lookup.
- **p95**: 95% of cached lookups were at or below this. A large gap from the median means occasional stalls.
- **jitter**: standard deviation of the cached lookups. Lower is smoother.
- **DNSSEC**: `yes`, `no`, or `?` when the check could not be completed.

`--top 10` limits the tables to the first ten rows. It does not affect the summary, the JSON or the CSV.

## Use the winner

dnsrace doesn't change anything itself. Put the recommended server first and a runner-up from a different operator second.

**Linux** (NetworkManager):

```bash
nmcli -t -f NAME,DEVICE con show --active    # find your connection's name
nmcli con mod "<name>" ipv4.ignore-auto-dns yes ipv4.dns "1.1.1.1 1.0.0.1"
nmcli con up "<name>"
resolvectl dns                               # check which servers are in use
```

On KDE or GNOME the same setting is in the network settings of your connection (IPv4 tab; on KDE choose "Automatic (only addresses)" so the router's DNS isn't mixed in). If `resolvectl dns` still lists an extra IPv6 server from your router, also add `ipv6.ignore-auto-dns yes`.

**macOS**:

```bash
networksetup -listallnetworkservices
sudo networksetup -setdnsservers Wi-Fi 1.1.1.1 1.0.0.1     # undo: ... -setdnsservers Wi-Fi Empty
```

**Windows** (PowerShell as Administrator):

```powershell
Get-NetAdapter                                             # find the adapter's name
Set-DnsClientServerAddress -InterfaceAlias "Wi-Fi" -ServerAddresses ("1.1.1.1","1.0.0.1")
# undo: Set-DnsClientServerAddress -InterfaceAlias "Wi-Fi" -ResetServerAddresses
```

You can also set DNS on your router so every device benefits.

## What DNS can and can't do

DNS only decides how long name lookups take and which server IP a site hands you. It won't raise your download speed or lower your in-game ping by itself.

If one site is slow while speed tests look fine, run `dnsrace that-site.com`. If every server returns the same addresses, DNS isn't the cause: the bottleneck is the route between your ISP and that site. Each address is also timed with TCP connections, so you can see when one of them is unreachable or only intermittently reachable.

## Scripting: quiet, JSON and CSV

```bash
dnsrace -q                      # 1.1.1.1
dnsrace --json > result.json
dnsrace --csv > result.csv
```

`-q` prints exactly one line on stdout: the recommended address. It is meant for shell scripts. Use `--json` if you also need the numbers.

`dnsrace --json` prints one JSON document on stdout and all human-readable messages on stderr:

```json
{
  "dnsrace": "0.5.0",
  "data_center": "mrs04",
  "results": [
    {"rank": 1, "server": "1.1.1.1", "name": "Cloudflare", "cached_ms": 45.0, "uncached_ms": 66.0, "min_ms": 40.2, "p95_ms": 51.8, "jitter_ms": 2.4, "timeouts": 0, "flag": null, "dnssec": true, "current": false}
  ],
  "recommended": "1.1.1.1",
  "fastest": "1.1.1.1",
  "fastest_known": "1.1.1.1",
  "steering": null,
  "steerings": []
}
```

`flag` is `null`, `"HIJACKS-NXDOMAIN"` or `"UNRELIABLE"`. `dnssec` is `true`, `false` or `null` when the check could not be completed. `recommended` is the address from the summary; `fastest` and `fastest_known` are kept for compatibility. With hostnames, `steering` holds the first result and `steerings` holds every result in the order given; each has `host`, `servers` and a list of `addresses` (`ip`, `returned_by`, `connect_ms`, `failed`).

CSV columns:

```
rank,server,name,cached_ms,uncached_ms,min_ms,p95_ms,jitter_ms,timeouts,flag,dnssec,current
```

Empty cells mean "not measured". `--csv` lists the servers only; the `HOSTNAME` steering check is skipped with a note on stderr.

## The server list

The built-in list is [`servers.txt`](servers.txt), one `address name` per line:

```
1.1.1.1 Cloudflare
2606:4700:4700::1111 Cloudflare
127.0.0.1:5353 My-local-resolver
```

Addresses are IPv4 or IPv6, with an optional port (`1.1.1.1:5353`, `[2606:4700:4700::1111]:5353`). Servers that can't be reached from your connection (IPv6 on an IPv4-only network, for example) are dropped automatically, so listing them is harmless. The list is compiled into the binary, so rebuild after editing it, or test a list without rebuilding using `dnsrace --servers servers.txt`.

To add a server, append a line and open a pull request. Please only add public resolvers run by an identifiable operator, with a page that says so.

## Platform support

| | Linux | macOS | Windows |
|---|---|---|---|
| Testing servers (IPv4 and IPv6) | yes | yes | yes |
| Router detection | `/proc/net/route` | `route` | PowerShell |
| Current DNS detection | `resolv.conf`, `resolvectl`, `nmcli` | `scutil`, `resolv.conf` | PowerShell |

Detection is best effort: if it can't find your router or your current DNS, everything else still works, and `--no-detect` skips it. On Windows it starts PowerShell, which takes about a second.

## Troubleshooting

- **"no server answered"**: a firewall, VPN or captive portal may be blocking DNS on UDP port 53. Check your connection, and try `--timeout 3000` on a slow link.
- **The top servers are within a millisecond**: run `dnsrace --repeat 3`. DNS timings vary with the time of day, so a second run at a busy hour is also useful.
- **Every server shows almost the same time**: your network may be redirecting all DNS traffic to its own resolver, so the numbers say little about the servers you picked.
- **IPv6 timeouts**: some networks advertise IPv6 without routing it. Use `--ipv4` to skip those candidates.
- **`--nearby` found no country**: use `--countries <cc>` explicitly (for example `--countries eg`).
- **`--countries` takes longer for a big country**: some countries list thousands of open resolvers. They are all scanned (once each), so expect an extra minute or so; `--other` controls how many get the full test.
- **VPN on**: you are measuring the path through the VPN, not your own connection.

## How it works

dnsrace builds the DNS packets itself, sends them over UDP and times the replies with microsecond precision, so nothing is spawned per lookup.

1. **Quick scan**: one usable answer from every candidate, up to 64 at a time, retrying once for listed servers. The per-lookup timeout is `--timeout`, capped at 3 seconds for this stage. Servers that don't answer drop out.
2. **Full test** of the fastest ones, 16 servers at a time: an untimed warm-up pass, then 30 cached lookups plus 5 uncached lookups (random names under `.com`) per server, plus one DNSSEC probe. Servers that fail too many lookups are flagged, and servers that answer non-existent names with a fake address are flagged too. `--repeat` pools several rounds.
3. **Ranking and statistics**: the score is the average of the cached and uncached medians; min, p95 and jitter are calculated from the cached samples. Then comes the summary, the optional detail table, and optionally the steering check for the hostname you passed (the addresses the fastest servers hand out, and the TCP connect time to each).

## Safety and privacy

- Every server you test sees the test lookups: ten popular sites, random names, one DNSSEC probe for `cloudflare.com`, and the hostname you pass.
- Entries named `other` come from [public-dns.info](https://public-dns.info/), a list of open resolvers. Many are misconfigured home routers that can disappear, log your lookups, or return fake answers. They are included to show what's nearby; only switch to a resolver whose operator you trust. The list isn't always up to date, so dnsrace re-tests everything live.
- dnsrace sends no telemetry. It contacts the servers under test, Cloudflare (`1.1.1.1`) for the data-center check (skipped with `--no-detect`) and for the `--nearby` country lookup, public-dns.info (only with `--countries`/`--nearby`), and, if you pass a hostname, the site you named.

## Development

```bash
cargo fmt --all
cargo clippy --all-targets --features gui -- -D warnings
cargo test --features gui
```

The tests need no network: unit tests cover the DNS packets, parsing, statistics and reports, and end-to-end tests run the real binary against simulated DNS servers on loopback (fast, slow, hijacking, silent, dead, IPv6, DNSSEC, exports). CI runs the tests on Linux, build-checks macOS and Windows, and checks formatting and lints with Rust 1.99, the minimum supported version. Dependabot sends weekly update pull requests.

To publish a release, set the version in `Cargo.toml`, update `CHANGELOG.md`, commit, then push a matching tag such as `v0.5.0`. The release workflow builds native CLI and GUI binaries for Linux x86_64, Linux ARM64, Windows x86_64, macOS Intel and macOS Apple Silicon, then attaches them with one consolidated `SHA256SUMS.txt` to a GitHub release.

## License

MIT. See [LICENSE](LICENSE).
