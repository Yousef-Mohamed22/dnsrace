# dnsrace

> Race DNS servers from **your own connection** and find the fastest one for **your location**.

The "best" DNS server depends on where you are and how your ISP routes to each provider, so a top-10 list from the internet can't tell you which one is best for you. `dnsrace` measures it: it tests about 85 well-known public resolvers (IPv4 and IPv6, and optionally every open resolver listed for your country) and ranks them by real lookup speed from your machine.

- **Fast.** A single Rust binary that speaks DNS itself: servers are tested in parallel and a full run usually takes well under 30 seconds.
- **Nothing to install.** No `dig`, no `curl`, no runtime. Download one file and run it.
- **Works everywhere.** Linux, macOS and Windows, IPv4 and IPv6.
- **Safe.** It only measures. It never changes your system's DNS settings.

## Install

**Download a build** from the [Releases page](https://github.com/Yousef-Mohamed22/dnsrace/releases): Linux (x86_64 and arm64, static, runs on any distribution), macOS (Apple Silicon and Intel) and Windows (x86_64). Unpack it and run `dnsrace` (`dnsrace.exe` on Windows) from a terminal. Each release has a `SHA256SUMS.txt` to check your download.

- macOS: the binaries are not signed, so the first run may be blocked. Remove the quarantine flag once with `xattr -d com.apple.quarantine ./dnsrace`.
- Windows: SmartScreen may warn about an unsigned program; choose "More info" then "Run anyway", or run it from PowerShell: `.\dnsrace.exe`.

**Or build it yourself** with [Rust](https://rustup.rs) 1.88 or newer:

```bash
cargo install --git https://github.com/Yousef-Mohamed22/dnsrace
# or, from a clone:
cargo build --release          # the binary is target/release/dnsrace
```

## Usage

```bash
dnsrace                        # race the built-in server list
dnsrace --countries eg         # also test the open resolvers listed for Egypt
dnsrace github.com             # also show where github.com gets steered
dnsrace --json > result.json   # machine-readable results
```

| Option | What it does |
|---|---|
| `-c, --countries <CC,...>` | also test the open resolvers [public-dns.info](https://public-dns.info/) lists for these 2-letter country codes (e.g. `eg,it,fr`) |
| `-k, --known <N>` | fully test the N fastest servers from the list (default 30) |
| `-o, --other <N>` | fully test the N fastest country resolvers (default 10) |
| `-s, --servers <FILE>` | use your own server list instead of the built-in one |
| `-a, --add <ADDR,...>` | also test these servers (IPv4 or IPv6, optionally with `:port`) |
| `-j, --jobs <N>` | how many servers to fully test at once (default 16) |
| `--timeout <MS>` | per-lookup timeout in the full test (default 1000) |
| `--connect-port <PORT>` | TCP port for the connect-time check of the hostname (default 443) |
| `--no-detect` | don't look up your router, your current DNS servers or your Cloudflare data center |
| `--json` | print the results as JSON on stdout (progress goes to stderr) |
| `--no-color` | don't use colors |

`dnsrace --help` shows the same list. Your router and your current DNS servers are detected automatically, always tested, and marked `<== current`. Exit status: 0 on success, 1 if no server could be tested, 2 for a usage error.

## Reading the results

Example output (trimmed; the numbers are illustrative, based on a real run; yours will differ):

```
== Cloudflare data center you reach (first 3 letters = nearest city's airport code, e.g. cai = Cairo, mrs = Marseille, fra = Frankfurt) ==
mrs07

Your current DNS: 163.121.128.134 163.121.128.135 192.168.1.1

Stage 1: quick scan of 88 candidates...
53 answered.

Stage 2: full test of 30 servers (16 at a time)...
                                        
== Results: average of the cached and uncached medians, lower is better ==
 1. 192.168.1.1     Router/ISP-default   cached    1.1 ms  uncached   52.8 ms  timeouts 1  <== current
 2. 163.121.128.135 current              cached    3.9 ms  uncached   52.2 ms  timeouts 0  <== current
 3. 163.121.128.134 current              cached    3.8 ms  uncached   52.4 ms  timeouts 1  <== current
 4. 1.1.1.1         Cloudflare           cached   45.3 ms  uncached   62.9 ms  timeouts 0
 5. 1.0.0.1         Cloudflare           cached   45.8 ms  uncached   66.5 ms  timeouts 0
 6. 1.0.0.3         Cloudflare-family    cached   46.5 ms  uncached   65.9 ms  timeouts 0
 7. 1.0.0.2         Cloudflare-malware   cached   46.9 ms  uncached   66.7 ms  timeouts 0
 8. 8.8.8.8         Google               cached   51.7 ms  uncached   62.5 ms  timeouts 0
 9. 1.1.1.3         Cloudflare-family    cached   46.2 ms  uncached   68.2 ms  timeouts 0
10. 76.76.10.0      ControlD             cached   57.0 ms  uncached   58.1 ms  timeouts 1
11. 149.112.112.112 Quad9                cached   53.2 ms  uncached   62.1 ms  timeouts 0
12. 185.228.169.9   CleanBrowsing        cached   55.3 ms  uncached   61.2 ms  timeouts 7
13. 1.1.1.2         Cloudflare-malware   cached   46.5 ms  uncached   70.8 ms  timeouts 0
14. 185.228.168.168 CleanBrowsing-family cached   55.5 ms  uncached   65.6 ms  timeouts 7
15. 195.46.39.39    SafeDNS              cached   55.9 ms  uncached   66.2 ms  timeouts 0
16. 195.46.39.40    SafeDNS              cached   56.4 ms  uncached   68.6 ms  timeouts 0
17. 8.8.4.4         Google               cached   53.4 ms  uncached   73.3 ms  timeouts 0
18. 4.2.2.1         Level3               cached   62.5 ms  uncached   66.6 ms  timeouts 0
19. 209.244.0.4     Level3               cached   62.7 ms  uncached   69.8 ms  timeouts 0
20. 94.140.14.141   AdGuard-unfiltered   cached   62.2 ms  uncached   71.3 ms  timeouts 0
21. 94.140.14.140   AdGuard-unfiltered   cached   61.8 ms  uncached   74.0 ms  timeouts 0
22. 94.140.15.15    AdGuard              cached   61.6 ms  uncached   74.3 ms  timeouts 0
23. 94.140.14.14    AdGuard              cached   61.7 ms  uncached   80.0 ms  timeouts 1
24. 64.6.64.6       Verisign             cached   59.9 ms  uncached   87.1 ms  timeouts 0
25. 74.82.42.42     HurricaneElectric    cached   56.5 ms  uncached  133.2 ms  timeouts 0
26. 149.112.112.11  Quad9-ECS            cached   64.7 ms  uncached  135.4 ms  timeouts 0
27. 149.112.112.10  Quad9-unfiltered     cached   93.8 ms  uncached  142.5 ms  timeouts 0
28. 185.228.168.9   CleanBrowsing        cached   55.2 ms  uncached       n/a  timeouts 8  UNRELIABLE
29. 8.20.247.20     Comodo               cached   62.3 ms  uncached       n/a  timeouts 8  UNRELIABLE
30. 84.200.69.80    DNS.WATCH            cached   66.7 ms  uncached       n/a  timeouts 8  UNRELIABLE

Fastest overall:        192.168.1.1 (Router/ISP-default)
Fastest known provider: 1.1.1.1 (Cloudflare)
Your current DNS:       #1 (192.168.1.1), #2 (163.121.128.135), #3 (163.121.128.134) of 30 tested

A DNS server sees every site you visit and can return fake answers, so only switch to one you trust.
How to switch: see 'Use the winner' in the README.
Done in 13.3s.

```

- **cached**: median of 30 lookups of popular sites. Mostly measures how far away the server is.
- **uncached**: median of 5 lookups of random names that nobody has cached. Shows how good the server's own connection to the rest of the internet is. This is the fair number to compare against your router, which wins "cached" simply because it sits on your LAN.
- **timeouts**: lookups that got no answer (out of 35). More than a few means an unreliable server.
- Servers are ranked by the average of the two medians. Lower is better.
- `HIJACKS-NXDOMAIN`: the server returns a fake answer for names that don't exist (usually an ad or redirect page). Don't use it.
- `UNRELIABLE`: eight or more failed lookups. These always rank last, even if their other numbers look good.
- `Router/ISP-default` is your gateway, which normally forwards to your ISP's DNS. `other` is an open resolver from public-dns.info (see [Safety and privacy](#safety-and-privacy)).

**Fastest known provider** is the best result from the built-in list, which is usually the one you want.

## Use the winner

dnsrace doesn't change anything itself. Put the fastest server first and the runner-up from a different operator second.

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

## JSON output

`dnsrace --json` prints one JSON document on stdout and all progress text on stderr:

```json
{
  "dnsrace": "0.3.0",
  "data_center": "mrs04",
  "results": [
    {"rank": 1, "server": "192.168.1.1", "name": "Router/ISP-default", "cached_ms": 5.0, "uncached_ms": 54.0, "timeouts": 0, "flag": null, "current": false}
  ],
  "fastest": "192.168.1.1",
  "fastest_known": "1.1.1.1",
  "steering": null
}
```

`flag` is `null`, `"HIJACKS-NXDOMAIN"` or `"UNRELIABLE"`. With a hostname, `steering` holds `host`, `servers` and a list of `addresses` (`ip`, `returned_by`, `connect_ms`, `failed`).

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
- **Every server shows almost the same time**: your network may be redirecting all DNS traffic to its own resolver, so the numbers say little about the servers you picked.
- **Results change between runs**: DNS timings vary with the time of day. Run it a few times, including at your busiest hours.
- **VPN on**: you are measuring the path through the VPN, not your own connection.
- **`--countries` takes longer for a big country**: some countries list thousands of open resolvers. They are all scanned (once each), so expect an extra minute or so; `--other` controls how many of them get the full test.

## How it works

dnsrace builds the DNS packets itself, sends them over UDP and times the replies with microsecond precision, so nothing is spawned per lookup.

1. **Quick scan**: one lookup against every candidate, up to 64 at a time. Servers that don't answer drop out.
2. **Full test** of the fastest ones, 16 servers at a time: 30 cached lookups plus 5 uncached lookups (random names under `.com`) per server, one after another. Servers that fail too many lookups are flagged, and servers that answer non-existent names with a fake address are flagged too.
3. **Ranking**, a summary, and optionally the steering check for the hostname you passed (the addresses the fastest servers hand out, and the TCP connect time to each).

## Safety and privacy

- Every server you test sees the test lookups: ten popular sites, random names, and the hostname you pass.
- Entries named `other` come from [public-dns.info](https://public-dns.info/), a list of open resolvers. Many are misconfigured home routers that can disappear, log your lookups, or return fake answers. They are included to show what's nearby; only switch to a resolver whose operator you trust. The list isn't always up to date, so dnsrace re-tests everything live.
- dnsrace sends no data anywhere except the lookups themselves. It contacts the servers under test, Cloudflare (`1.1.1.1`) for the data-center check (skipped with `--no-detect`), public-dns.info (only if you use `--countries`), and, if you pass a hostname, the site you named.

## Development

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

The tests need no network: unit tests cover the DNS packets, parsing and reports, and end-to-end tests run the real binary against simulated DNS servers on loopback (fast, slow, hijacking, silent, dead, IPv6). CI runs the tests on Linux, macOS and Windows, and checks formatting and lints with a pinned compiler (`LINT_TOOLCHAIN` in `.github/workflows/ci.yml`) so a newer compiler can't break the build on unchanged code. Dependabot sends weekly update pull requests.

To publish a release, set the version in `Cargo.toml`, commit, then push a matching tag such as `v0.3.0`. The release workflow builds the binaries and attaches them, with checksums, to a GitHub release.

## License

MIT. See [LICENSE](LICENSE).
