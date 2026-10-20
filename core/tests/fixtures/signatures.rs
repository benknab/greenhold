/// `signer/` signatures using the public BIP-39 test mnemonic
/// ("abandon" x 11, "about") at the default BIP-44/49/84/86 paths.
pub const MESSAGE: &str = "Greenhold BIP-322 test";
pub const P2PKH: &str = "1LqBGSKuX5yYUonjxT5qGfpUsXKYYWeabA";
pub const P2SH_P2WPKH: &str = "37VucYSaXLCAsxYyAPfbSi9eh4iEcbShgf";
pub const P2WPKH: &str = "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu";
pub const P2TR: &str = "bc1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqkedrcr";

pub const P2PKH_FULL: &str = "fulAAAAAAGvr5khuRZ6fgtuYJvx+pCMFU9EEkNNeeOasUye6PpIaQAAAABqRzBEAiB22eW98WaDnoDOCBUPT/wyDm4wJg0hM66MMGKAnhOt2AIgWg6Y1xhyzWylrkHhsutDrWOnrsvVE4ZQpWD/bAdT/nUBIQOq61LddJTDYQSd5nzGgOg+vLu9vrE2N9ks2EX3AwivXgAAAAABAAAAAAAAAAABagAAAAA=";
pub const P2PKH_LEGACY: &str =
    "IGRRY1BMFgP74HM4ioCCViFls+IVON7/EGtXczd/crgybuIYMQx/Fo95lQBPdgCKJeh7JixdmDv7SJNiqCZNNAE=";
pub const P2SH_P2WPKH_FULL: &str = "fulAAAAAAABASpPoucl6zlLatw5fSidJDZLPIYEu44aVqW4ACyNnnZPAAAAABcWABT5kGeayv4lwnYVNztAvyJEbST/RAAAAAABAAAAAAAAAAABagJIMEUCIQCnVJ/CE3IyqKCxacFhvxFj9Ju6rfIHqk7Y3NASJ4mfEQIgVKOl0r4bxYtue+BysMNflOhLl2y2fKYa0q/JOeGUGsQBIQObO2lLj8W14H+waceDysdU9dOMPgi+0ZYOMf2x3aNcJAAAAAA=";
pub const P2WPKH_SIMPLE: &str = "smpAkgwRQIhAKhSHOe4EaSt0a4mn+BQdQGuirn2vIn/a2Z4R6YplK0AAiBG+iFd4tbWCVyT0q8daaBXVfss7lHhpOVsr+KWLgF1CgEhAzDVT9DdQgpuX402JPXzSCyuNQ951fB1O/W+75wtka88";
pub const P2WPKH_FULL: &str = "fulAAAAAAABAcfeb2Y11NVvOlbwX3xC3pKI7ifTj6V6DrqUa0DX5lnyAAAAAAAAAAAAAQAAAAAAAAAAAWoCSDBFAiEAqFIc57gRpK3Rriaf4FB1Aa6Kufa8if9rZnhHpimUrQACIEb6IV3i1tYJXJPSrx1poFdV+yzuUeGk5Wyv4pYuAXUKASEDMNVP0N1CCm5fjTYk9fNILK41D3nV8HU79b7vnC2RrzwAAAAA";
pub const P2TR_SIMPLE: &str =
    "smpAUBKXUxMoibYZeMAVZ8ky7yyyDsB4LJ2MP3c7XGmoPV0/9uImqM6jfs7yBPM1pL8IVWgN84C2HlOXGRjlCZ1fmE9";
pub const P2TR_FULL: &str = "fulAAAAAAABAfH6D7+nU2gqObyAXAj5lpi1ccS4ZUNg4yqcf5EwmHBeAAAAAAAAAAAAAQAAAAAAAAAAAWoBQKtTLiFIZBfqXfwSbYC40ZaUAeylT6wGzRZtF483HJgcElrcjczCHYkCA9IzVOXuM1MyyJ/qMKoFarxaIkTyWRsAAAAA";

pub const CASES: [(&str, &str); 7] = [
    (P2PKH, P2PKH_FULL),
    (P2PKH, P2PKH_LEGACY),
    (P2SH_P2WPKH, P2SH_P2WPKH_FULL),
    (P2WPKH, P2WPKH_SIMPLE),
    (P2WPKH, P2WPKH_FULL),
    (P2TR, P2TR_SIMPLE),
    (P2TR, P2TR_FULL),
];
