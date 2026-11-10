use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("underbox_vpk_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample() -> Builder {
    let mut b = Builder::new();
    b.add("maps/a/world.vwrld_c", b"world".to_vec())
        .add("Maps\\A\\Entities\\default_ents.vents_c", vec![7u8; 3000])
        .add("readme", b"no extension".to_vec())
        .add("top.txt", b"root dir".to_vec())
        .add("empty.bin", Vec::new());
    b
}

fn check(vpk: &Vpk) {
    assert_eq!(vpk.read("maps/a/world.vwrld_c").unwrap().unwrap(), b"world");
    assert_eq!(vpk.read("/MAPS/a/entities/default_ents.vents_c").unwrap().unwrap(), vec![7u8; 3000]);
    assert_eq!(vpk.read("readme").unwrap().unwrap(), b"no extension");
    assert_eq!(vpk.read("top.txt").unwrap().unwrap(), b"root dir");
    assert_eq!(vpk.read("empty.bin").unwrap().unwrap(), b"");
    assert!(vpk.read("maps/a/missing").is_none());
    assert_eq!(vpk.names().count(), 5);
}

#[test]
fn single_file_paks_round_trip() {
    let dir = temp("single");
    let path = dir.join("a.vpk");
    std::fs::write(&path, sample().single()).unwrap();
    check(&Vpk::open(&path).unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn chunked_paks_with_preload_round_trip() {
    let dir = temp("chunked");
    let path = sample().chunked(&dir, "pak01", 1000, 4).unwrap();
    assert!(dir.join("pak01_001.vpk").is_file(), "files after the 3000-byte one go to a second chunk");
    check(&Vpk::open(&path).unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rejects_corrupt_paks() {
    let dir = temp("corrupt");
    let path = dir.join("a.vpk");
    std::fs::write(&path, b"not a pak at all").unwrap();
    assert!(matches!(Vpk::open(&path), Err(Error::Signature(_))));
    let mut bytes = sample().single();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&path, &bytes).unwrap();
    let vpk = Vpk::open(&path).unwrap();
    let corrupt = vpk.names().find(|n| vpk.read(n).unwrap().is_err()).map(str::to_owned);
    assert!(corrupt.is_some(), "flipping the last data byte breaks one file's CRC");
    std::fs::write(&path, &bytes[..40]).unwrap();
    assert!(Vpk::open(&path).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
