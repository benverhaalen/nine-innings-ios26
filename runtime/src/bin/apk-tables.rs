//! Developer check: derive the runtime tables from an APK and write them out.
#[path="../apk.rs"] mod apk;
fn main(){
 let args:Vec<String>=std::env::args().collect();
 let bytes=std::fs::read(&args[1]).expect("usage: apk-tables <apk> <out-dir>");
 println!("sha256 {} (supported: {})",apk::sha256_hex(&bytes),apk::sha256_hex(&bytes)==apk::SUPPORTED_APK_SHA256);
 let t=apk::derive(&bytes).unwrap();let out=std::path::Path::new(&args[2]);std::fs::create_dir_all(out).unwrap();
 std::fs::write(out.join("libgame.so"),&t.engine).unwrap();std::fs::write(out.join("apk-assets.tsv"),&t.assets).unwrap();
 std::fs::write(out.join("declared-methods.tsv"),&t.methods).unwrap();std::fs::write(out.join("apk-string-resources.tsv"),&t.strings).unwrap();
}
