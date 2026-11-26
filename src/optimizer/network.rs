//! Network > Test: pings the router and a public server (Cloudflare's 1.1.1.1) through Windows' ICMP API and
//! reports ping, jitter and packet loss, so problems inside the home network can be told apart from the
//! internet connection. Read-only.

use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, ICMP_ECHO_REPLY, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN};

/// The public server pinged: Cloudflare's DNS, close to most players.
pub const INTERNET: ([u8; 4], &str) = ([1, 1, 1, 1], "Internet (1.1.1.1)");
const PINGS: usize = 25;
const GAP_MS: u64 = 150;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub sent: usize,
    pub received: usize,
    pub avg_ms: f64,
    pub max_ms: u32,
    /// Average change between consecutive replies.
    pub jitter_ms: f64,
}

impl Stats {
    pub fn loss_pct(&self) -> f64 {
        if self.sent == 0 {
            0.0
        } else {
            100.0 * (self.sent - self.received) as f64 / self.sent as f64
        }
    }
}

/// Summary of one run of pings; `None` is a lost packet.
pub fn summarize(samples: &[Option<u32>]) -> Stats {
    let got: Vec<u32> = samples.iter().flatten().copied().collect();
    let avg = if got.is_empty() { 0.0 } else { got.iter().map(|&v| f64::from(v)).sum::<f64>() / got.len() as f64 };
    let diffs: Vec<f64> = got.windows(2).map(|w| (f64::from(w[1]) - f64::from(w[0])).abs()).collect();
    let jitter = if diffs.is_empty() { 0.0 } else { diffs.iter().sum::<f64>() / diffs.len() as f64 };
    Stats { sent: samples.len(), received: got.len(), avg_ms: avg, max_ms: got.iter().copied().max().unwrap_or(0), jitter_ms: jitter }
}

fn ping(ip: [u8; 4]) -> Result<Stats, String> {
    let addr = u32::from_ne_bytes(ip);
    let data = [0x4bu8; 32];
    let size = std::mem::size_of::<ICMP_ECHO_REPLY>() + data.len() + 8;
    let mut samples = Vec::with_capacity(PINGS);
    unsafe {
        let h = IcmpCreateFile().map_err(|e| e.message())?;
        for i in 0..PINGS {
            let mut buf = vec![0u64; size.div_ceil(8)];
            let n = IcmpSendEcho(h, addr, data.as_ptr().cast(), data.len() as u16, None, buf.as_mut_ptr().cast(), size as u32, 1000);
            let reply = &*(buf.as_ptr() as *const ICMP_ECHO_REPLY);
            samples.push((n > 0 && reply.Status == 0).then_some(reply.RoundTripTime));
            if i + 1 < PINGS {
                std::thread::sleep(std::time::Duration::from_millis(GAP_MS));
            }
        }
        let _ = IcmpCloseHandle(h);
    }
    Ok(summarize(&samples))
}

/// The IPv4 default gateway (router) of the first connected adapter that has one.
fn gateway() -> Option<[u8; 4]> {
    unsafe {
        let mut size = 0u32;
        let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let _ = GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, None, &mut size);
        let mut buf = vec![0u64; (size as usize).div_ceil(8).max(1)];
        let first = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        if GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(first), &mut size) != 0 {
            return None;
        }
        let mut a = first;
        while !a.is_null() {
            let ad = &*a;
            if ad.OperStatus == IfOperStatusUp {
                let mut g = ad.FirstGatewayAddress;
                while !g.is_null() {
                    let sa = (*g).Address.lpSockaddr;
                    if !sa.is_null() && (*sa).sa_family == AF_INET {
                        let sin = &*(sa as *const SOCKADDR_IN);
                        return Some(sin.sin_addr.S_un.S_addr.to_ne_bytes());
                    }
                    g = (*g).Next;
                }
            }
            a = ad.Next;
        }
    }
    None
}

#[derive(Clone, Debug)]
pub struct NetResult {
    /// (label, stats) for the router, when one was found.
    pub router: Option<(String, Stats)>,
    pub internet: Stats,
}

/// Pings the router and the internet at the same time; takes about four seconds.
pub fn run() -> Result<NetResult, String> {
    let gw = gateway();
    std::thread::scope(|s| {
        let router = gw.map(|ip| s.spawn(move || ping(ip).map(|st| (format!("Router ({}.{}.{}.{})", ip[0], ip[1], ip[2], ip[3]), st))));
        let internet = ping(INTERNET.0)?;
        let router = router.and_then(|h| h.join().ok()).transpose()?;
        Ok(NetResult { router, internet })
    })
}

/// What the numbers mean, in plain words.
pub fn verdict(r: &NetResult) -> Vec<String> {
    let mut out = Vec::new();
    let net = &r.internet;
    let home = r.router.as_ref().map(|(_, s)| s);
    if net.received == 0 {
        out.push("No replies from the internet. Check that you're online; some networks block ping.".into());
        return out;
    }
    if net.loss_pct() > 0.0 {
        let local = home.is_some_and(|h| h.loss_pct() > 0.0);
        out.push(if local {
            format!(
                "{:.0}% of packets were lost, already inside your home network: usually Wi-Fi interference or a loose cable.",
                net.loss_pct()
            )
        } else {
            format!("{:.0}% of packets were lost beyond your router: usually your internet provider or a busy line.", net.loss_pct())
        });
    }
    if net.jitter_ms > 10.0 {
        let local = home.is_some_and(|h| h.jitter_ms > 5.0);
        out.push(if local {
            "Ping jumps around inside your home network: Wi-Fi interference or others using the network. Ethernet fixes most of this."
                .into()
        } else {
            "Ping jumps around beyond your router: your provider or the route to the server. Local settings can't fix that.".into()
        });
    }
    if out.is_empty() {
        out.push("Your connection is steady: no lost packets and little jitter.".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_loss_and_jitter() {
        let s = summarize(&[Some(10), Some(20), None, Some(10)]);
        assert_eq!((s.sent, s.received, s.max_ms), (4, 3, 20));
        assert!((s.avg_ms - 13.333).abs() < 0.01);
        assert!((s.jitter_ms - 10.0).abs() < 0.01);
        assert!((s.loss_pct() - 25.0).abs() < 0.01);
        assert_eq!(summarize(&[]).loss_pct(), 0.0);
    }

    #[test]
    fn verdict_places_the_problem() {
        let steady = Stats { sent: 25, received: 25, avg_ms: 12.0, max_ms: 15, jitter_ms: 1.0 };
        let lossy = Stats { received: 23, ..steady };
        let wifi = Stats { jitter_ms: 8.0, ..steady };
        let r = |router: Stats, internet: Stats| NetResult { router: Some(("Router".into(), router)), internet };
        assert!(verdict(&r(steady, steady))[0].contains("steady"));
        assert!(verdict(&r(lossy, lossy))[0].contains("inside your home network"));
        assert!(verdict(&r(steady, lossy))[0].contains("beyond your router"));
        assert!(verdict(&r(wifi, Stats { jitter_ms: 15.0, ..steady }))[0].contains("Ethernet"));
    }

    #[test]
    #[ignore = "pings the router and 1.1.1.1"]
    fn live_run() {
        let r = run().unwrap();
        println!("{r:?}");
        println!("{:?}", verdict(&r));
    }
}
