fn main() {
    let result = (|| -> bili_core::Res<()> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if args.len() != 3 {
            return bili_core::fail("用法：verify_update <zip> <sig> <version>");
        }
        let bytes = std::fs::read(&args[0])?;
        let signature = std::fs::read_to_string(&args[1])?;
        bili_core::update::verify(&bytes, &signature, bili_core::update::update_key(None), &args[2])?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
