use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let musashi_dir = manifest_dir.join("../third_party/musashi");
    let include_dir = manifest_dir.join("include");
    let softfloat_dir = musashi_dir.join("softfloat");
    let generated_ops_c = out_dir.join("m68kops.c");
    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let ar = env::var("AR").unwrap_or_else(|_| "ar".to_string());
    let musashi_conf_define = "-DMUSASHI_CNF=\"mini68k_m68kconf.h\"";

    let sources = [
        musashi_dir.join("m68kcpu.c"),
        musashi_dir.join("m68kdasm.c"),
        generated_ops_c.clone(),
        softfloat_dir.join("softfloat.c"),
    ];

    let extra_deps = [
        musashi_dir.join("m68k.h"),
        musashi_dir.join("m68kcpu.h"),
        musashi_dir.join("m68kops.h"),
        musashi_dir.join("m68kfpu.c"),
        musashi_dir.join("m68kmake.c"),
        musashi_dir.join("m68k_in.c"),
        musashi_dir.join("m68kmmu.h"),
        softfloat_dir.join("softfloat.h"),
        include_dir.join("mini68k_m68kconf.h"),
        include_dir.join("mini68k_musashi_bridge.h"),
    ];

    for dep in sources.iter().chain(extra_deps.iter()) {
        println!("cargo:rerun-if-changed={}", dep.display());
    }

    let generator = out_dir.join("m68kmake");
    let mut build_generator = Command::new(&cc);
    build_generator
        .arg("-std=c99")
        .arg("-O2")
        .arg(musashi_dir.join("m68kmake.c"))
        .arg("-o")
        .arg(&generator);
    run(
        &mut build_generator,
        "failed to compile Musashi opcode generator",
        &manifest_dir,
    );

    fs::copy(musashi_dir.join("m68k_in.c"), out_dir.join("m68k_in.c"))
        .expect("copy m68k_in.c to OUT_DIR");
    let mut generate_tables = Command::new(&generator);
    run(
        &mut generate_tables,
        "failed to generate Musashi opcode tables",
        &out_dir,
    );

    let mut objects = Vec::new();
    for source in &sources {
        let object_name = source
            .strip_prefix(&musashi_dir)
            .unwrap_or(source)
            .display()
            .to_string()
            .replace('/', "_");
        let object_path = out_dir.join(format!("{object_name}.o"));

        let mut command = Command::new(&cc);
        command
            .arg("-std=c99")
            .arg("-O2")
            .arg(musashi_conf_define)
            .arg("-I")
            .arg(&out_dir)
            .arg("-I")
            .arg(&musashi_dir)
            .arg("-I")
            .arg(&include_dir)
            .arg("-I")
            .arg(&softfloat_dir)
            .arg("-c")
            .arg(source)
            .arg("-o")
            .arg(&object_path);

        run(
            &mut command,
            &format!("failed to compile {}", source.display()),
            &manifest_dir,
        );
        objects.push(object_path);
    }

    let library_path = out_dir.join("libmusashi.a");
    let mut archive = Command::new(&ar);
    archive.arg("crs").arg(&library_path);
    for object in &objects {
        archive.arg(object);
    }
    run(
        &mut archive,
        "failed to archive Musashi objects",
        &manifest_dir,
    );

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=musashi");
}

fn run(command: &mut Command, message: &str, cwd: &Path) {
    let status = command.current_dir(cwd).status().unwrap_or_else(|error| {
        panic!("{message}: {error}");
    });
    if !status.success() {
        panic!("{message}: command exited with {status}");
    }
}
