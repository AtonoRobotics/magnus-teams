//! The owner, 2026-11-27: "Dismiss the old duplicate observations". The
//! reviews had queued 271 open observations, most of them the same news
//! told again. Read by hand, one of each piece of news was kept: these.
//! Every other observation or interruption queued before
//! `BACKLOG_READ_AT` is dismissed once, at boot.

/// When the backlog was read: after the newest item in it, 22:02.
pub const BACKLOG_READ_AT: &str = "2026-11-27T22:30:00Z";

/// The newest of each piece of news in the backlog.
pub const KEPT_NEWS: [&str; 27] = [
    "item_a0ee3d3fd2580cc503a6cd472990aec4",
    "item_d57344f522aeac3600616739629b4691",
    "item_8e440fdfc88eed39d9f6d6b9dd7c844e",
    "item_f188bb14e2ec0ecef667372c028b0dde",
    "item_255d0f35027b387a256f20517249fbea",
    "item_5c4f0a83e234668cc2932ec5e4bf9bd4",
    "item_00f01366116ee1b594cf98df247ddab8",
    "item_3f469e301246d2459e02b22f00fb4a1a",
    "item_d51f2e04b0baea9866f53194a577a356",
    "item_316620eca2245b71e08255a4691e6304",
    "item_9164991a8ccb7bb300987b6831589acc",
    "item_6fc3c748d2b2305a6b74a769caea67b2",
    "item_a3ede6ef5d882e95e8100ce9ac00352c",
    "item_b863712ffd937cbd64ff69edf69ad0ee",
    "item_a60823133b1a22b96c1a1cc858702b04",
    "item_8298e66977f0ea2e1ac89eb4d7e64d00",
    "item_30f48c1c2548c547374d86fde3319a68",
    "item_944ea72ce091f3f923666634713f2fdb",
    "item_0e4b8572e32d66d5da00d61d93b6488b",
    "item_3db15cdce86b08c3b36f5b0c9b09028c",
    "item_b9cb5d38706f414c9349fb0662a3d7f5",
    "item_d9611973ad970514beb6ae5d04edd5e1",
    "item_8ecede87ecf00fe5709dc4d05152ef9a",
    "item_d9f29a1d331c517c3cf2076aadb29f6a",
    "item_4467b101b6c3503ef08ab033861bfbd0",
    "item_e4d6887b87a8a4c7b6fed25b28a4ad6c",
    "item_d2c81d33e429f790616ab522f368e51f",
];
