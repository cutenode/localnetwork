### Fixed

- Plugin fetches, the browser opener and Atmosphere sign-in refuse a few more addresses that
  aren't public: carrier-grade NAT's 100.64.0.0/10 (which Tailscale uses), 0.0.0.0/8, the
  broadcast address, and NAT64 and 6to4 addresses that carry a local IPv4 address.
