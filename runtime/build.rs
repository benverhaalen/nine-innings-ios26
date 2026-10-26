use std::{env,path::PathBuf,process::Command};
fn main(){
 println!("cargo:rerun-if-changed=ios-graphics.m");
 let target=env::var("TARGET").unwrap();if !target.contains("apple-ios"){return}
 let sim=target.ends_with("-sim");let sdk=if sim{"iphonesimulator"}else{"iphoneos"};
 let output=Command::new("xcrun").args(["--sdk",sdk,"--show-sdk-path"]).output().unwrap();assert!(output.status.success());let sdkpath=String::from_utf8(output.stdout).unwrap();
 let dir=PathBuf::from(env::var("OUT_DIR").unwrap());let obj=dir.join("ios-graphics.o");let lib=dir.join("libiosgraphics.a");
 let triple=if sim{"arm64-apple-ios15.0-simulator"}else{"arm64-apple-ios15.0"};
 assert!(Command::new("xcrun").args(["--sdk",sdk,"clang","-target",triple,"-isysroot",sdkpath.trim(),"-fobjc-arc","-Wno-deprecated-declarations","-c","ios-graphics.m","-o"]).arg(&obj).status().unwrap().success());
 assert!(Command::new("ar").arg("crs").arg(&lib).arg(&obj).status().unwrap().success());
 println!("cargo:rustc-link-search=native={}",dir.display());println!("cargo:rustc-link-lib=static=iosgraphics");println!("cargo:rustc-link-lib=framework=Foundation");println!("cargo:rustc-link-lib=framework=OpenGLES");println!("cargo:rustc-link-lib=framework=QuartzCore");println!("cargo:rustc-link-lib=framework=UniformTypeIdentifiers");
}
