# dnsrace

> Race DNS servers from **your own connection** and find the fastest one for **your location**.

The "best" DNS server depends on where you are and how your ISP routes to each provider, so a top-10 list from the internet can't tell you which one is best for you. `dnsrace` measures it: it tests about 55 well-known public resolvers (and, optionally, the open resolvers listed for your country) and ranks them by real lookup speed from your machine.

It is a single fast Rust binary with no runtime dependencies (no `dig`, no `curl`), and it only measures: it never changes your system's DNS settings.

## Quick start

```bash
# 1. install Rust (one time)
sudo pacman -S --needed rust     # Arch / CachyOS / Manjaro
sudo apt install cargo           # Debian / Ubuntu (if it is too old, use https://rustup.rs)
sudo dnf install cargo           # Fedora

# 2. get and build dnsrace (a couple of minutes the first time)
git clone https://github.com/Yousef-Mohamed22/dnsrace.git
cd dnsrace
cargo build --release

# 3. race the servers (usually well under 30 seconds)
./target/release/dnsrace
```

To install it for everyday use: `cargo install --path .` (puts `dnsrace` in `~/.cargo/bin`) or `install -Dm755 target/release/dnsrace ~/.local/bin/dnsrace`.

Developed and tested with Rust 1.91 on CachyOS (Arch Linux). Linux only for now, because the router is detected from `/proc/net/route`.

## Options

| I want to... | Run |
|---|---|
| also test the open resolvers listed for my country | `dnsrace --countries eg` (2-letter code; several are fine: `--countries eg,it,fr`) |
| see where a slow site gets steered | `dnsrace github.com` |
| fully test more of the built-in list | `dnsrace --known 60` |
| fully test more country resolvers | `dnsrace --countries eg --other 30` |
| test specific servers too | `dnsrace --add 1.2.3.4,5.6.7.8` |
| use my own server list | `dnsrace --servers my-servers.txt` |
| test more (or fewer) servers at once | `dnsrace --jobs 32` |
| give slow servers more time per lookup | `dnsrace --timeout 2000` (milliseconds) |

`dnsrace --help` lists everything. Your router and your current DNS servers are detected automatically, always tested, and marked `<== current`.

## Reading the results

Example output (trimmed; the numbers are illustrative, based on a real run from Egypt; yours will differ):

```
== Results: average of the cached and uncached medians, lower is better ==
 1. 192.168.1.1      Router/ISP-default   cached    5.0 ms  uncached   54.0 ms  timeouts 0
 ...
 3. 1.1.1.1          Cloudflare           cached   45.0 ms  uncached   66.0 ms  timeouts 0
 4. 8.8.4.4          Google               cached   50.0 ms  uncached   61.0 ms  timeouts 0
 ...
16. 2.56.220.2       Gcore                cached   54.0 ms  uncached   68.0 ms  timeouts 0  <== current
 ...
Fastest overall:        192.168.1.1 (Router/ISP-default)
Fastest known provider: 1.1.1.1 (Cloudflare)
Your current DNS:       #16 (2.56.220.2) of 26 tested
```

- **cached**: median of 30 lookups of popular sites. Mostly measures how far away the server is.
- **uncached**: median of 5 lookups of random names that nobody has cached. Shows how good the server's own connection to the rest of the internet is. This is the fair number to compare against your router, which wins "cached" simply because it sits on your LAN.
- **timeouts**: lookups that got no answer (out of 35). More than a few means an unreliable server.
- Servers are ranked by the average of the two medians. Lower is better.
- `HIJACKS-NXDOMAIN`: the server returns a fake answer for names that don't exist (usually an ad or redirect page). Don't use it.
- `UNRELIABLE`: eight or more failed lookups. These always rank last, even if their other numbers look good.
- `Router/ISP-default` is your gateway, which normally forwards to your ISP's DNS. `other` is an open resolver from public-dns.info (see [Safety and privacy](#safety-and-privacy)).

**Fastest known provider** is the best result from `servers.txt`, which is usually the one you want.

## Use the winner

dnsrace doesn't change anything itself. With NetworkManager (most desktop distributions):

```bash
nmcli -t -f NAME,DEVICE con show --active    # find your connection's name
nmcli con mod "<name>" ipv4.ignore-auto-dns yes ipv4.dns "1.1.1.1 1.0.0.1"
nmcli con up "<name>"
resolvectl dns                               # check which servers are in use
```

Put the fastest server first and the runner-up from a different operator second. The same setting lives in your desktop's network settings (KDE: System Settings -> Wi-Fi & Networking -> your connection -> IPv4, method "Automatic (only addresses)", then the DNS servers field). If `resolvectl dns` still lists an extra IPv6 server from your router, also add `ipv6.ignore-auto-dns yes`. Without systemd-resolved, check `cat /etc/resolv.conf` instead.

## What DNS can and can't do

DNS only decides how long name lookups take and which server IP a site hands you. It won't raise your download speed or lower your in-game ping by itself.

If one site is slow while speed tests look fine, run `dnsrace that-site.com`. If every server returns the same IPs, DNS isn't the cause: the bottleneck is the route between your ISP and that site.

## Adding servers

`servers.txt` is the whole built-in list, one `IPv4 name` per line:

```
1.1.1.1 Cloudflare
```

Append a line and open a pull request. The list is compiled into the binary, so rebuild after editing it, or test a list without rebuilding using `dnsrace --servers servers.txt`. Please only add public resolvers run by an identifiable operator, with a page that says so. Ideas welcome: IPv6 support, DoH/DoT timing, more operators, macOS support.

## How it works

dnsrace talks DNS itself: it builds the UDP packets, sends them and times the replies (microsecond precision), so nothing is spawned per lookup.

1. **Quick scan**: one lookup against every candidate, up to 64 at a time. Servers that don't answer drop out.
2. **Full test** of the fastest ones, 16 servers at a time: 30 cached lookups plus 5 uncached lookups (random names under `.com`) per server, one after another. Servers that fail too many lookups are flagged, and servers that answer non-existent names with a fake IP are flagged too.
3. **Ranking**, a summary, and optionally the steering test for the hostname you passed (the IPs the fastest servers hand out, and the TCP connect time to each).

Run the tests with `cargo test`.

## Safety and privacy

- Every server you test sees the test lookups: ten popular sites, random names, and the hostname you pass.
- Entries named `other` come from [public-dns.info](https://public-dns.info/), a list of open resolvers. Many are misconfigured home routers that can disappear, log your lookups, or return fake answers. They are included to show what's nearby; only switch to a resolver whose operator you trust. The list isn't always up to date, so dnsrace re-tests everything live.
- dnsrace sends no data anywhere except the lookups themselves. It contacts the servers under test, Cloudflare (`1.1.1.1`) for the data-center check, public-dns.info (only if you use `--countries`), and, if you pass a hostname, the site you named.

## The original bash version

`dnsrace.sh` is the first version of this tool, kept for machines without Rust. It needs `bash`, `dig` and `curl`, reads the same `servers.txt` from its own folder, and takes its options as environment variables (for example `COUNTRIES=eg bash dnsrace.sh`). It does the same tests one after another, so it is much slower.

## License

MIT. See [LICENSE](LICENSE).
