fn main() {
    let a: Vec<String> = std::env::args().collect();
    let archive = std::path::Path::new(&a[1]);
    let dest = std::path::Path::new(&a[2]);
    let bundle = apoc_modengine::analyze_archive(archive).unwrap();
    let r = apoc_modengine::stage_bundle(archive, &bundle, dest).unwrap();
    println!(
        "files={} bytes={} previews={}",
        r.files_written, r.bytes_written, r.previews_written
    );
}
