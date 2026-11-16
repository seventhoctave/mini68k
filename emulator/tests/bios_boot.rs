use mini68k_emu::machine::{Machine, MachineConfig, MachineModel, VdpChip};
use mini68k_emu::rom;
use mini68k_emu::tms9918a::Tms9918a;
use mini68k_emu::v9938::{ScreenMode, VideoFrame, V9938};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

const TEXT_FG: u8 = 0x0f;
const TEXT_BG: u8 = 0x04;

fn integration_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn bios_monitor_boots_and_accepts_commands() {
    let _guard = integration_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let repo_root = repo_root();
    build_bios(&repo_root, None);
    let mut machine = machine_for_rom(&repo_root, MachineModel::V1, VdpChip::V9938);

    assert!(machine.run_until_output_contains(">", 20_000_000));

    machine.enqueue_input_text("?\r");
    assert!(machine.run_until_output_contains("Dump Memory", 2_000_000));

    machine.enqueue_input_text("F 10000 41 0001\rD 10000 0001\r");
    assert!(machine.run_until_output_contains("010000 41", 4_000_000));
}

#[test]
fn v9938_text_console_renders_banner_and_scrolls() {
    let _guard = integration_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let repo_root = repo_root();
    build_bios(&repo_root, Some("V9938=1"));
    let font = std::fs::read(repo_root.join("bios/font.bin")).expect("read bios/font.bin");
    let mut machine = machine_for_rom(&repo_root, MachineModel::V2, VdpChip::V9938);

    assert!(machine.run_until_output_contains(">", 20_000_000));
    {
        let vdp = machine.v9938().expect("v2 has a V9938");
        assert_eq!(vdp.screen_mode(), ScreenMode::Text2);
        assert_text_row(vdp, &font, 0, "Mini 68008 BIOS");
        assert_text_row(vdp, &font, 1, ">");
        let frame = machine.video_frame().expect("v2 frame");
        assert_eq!((frame.width, frame.height), (512, 212));
        write_png(&repo_root, "v9938-text.png", &frame);
    }

    // 32 dump lines push the banner off the top of the 24-row screen.
    machine.enqueue_input_text("D 10000 0020\r");
    assert!(run_until(&mut machine, 40_000_000, |output| {
        output.contains("0101F0") && output.ends_with("\r\n>")
    }));
    let vdp = machine.v9938().unwrap();
    assert_text_row(vdp, &font, 23, ">");
    assert_text_row(vdp, &font, 22, "0101F0 ");
    assert_text_row(vdp, &font, 0, "0100");
    write_png(
        &repo_root,
        "v9938-scroll.png",
        &machine.video_frame().unwrap(),
    );
}

#[test]
fn v9938_graphics_demo_draws_bitmap_and_sprites() {
    let _guard = integration_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let repo_root = repo_root();
    build_bios(&repo_root, Some("V9938=1"));
    let font = std::fs::read(repo_root.join("bios/font.bin")).expect("read bios/font.bin");
    let mut machine = machine_for_rom(&repo_root, MachineModel::V2, VdpChip::V9938);
    assert!(machine.run_until_output_contains(">", 20_000_000));

    machine.enqueue_input_text("G\r");
    assert!(run_until(&mut machine, 40_000_000, |output| {
        output.matches('>').count() >= 2
    }));

    {
        let vdp = machine.v9938().unwrap();
        assert_eq!(vdp.screen_mode(), ScreenMode::Graphic4);
        assert_eq!(vdp.display_lines(), 212);
        let color = |index: u8| vdp.palette_rgb(index);

        // colour bars
        for c in 0..16u8 {
            assert_eq!(
                vdp.native_pixel(usize::from(c) * 16 + 8, 16),
                color(c),
                "bar {c}"
            );
        }
        // frame outline
        assert_eq!(vdp.native_pixel(100, 40), color(15));
        assert_eq!(vdp.native_pixel(100, 211), color(15));
        assert_eq!(vdp.native_pixel(0, 120), color(15));
        assert_eq!(vdp.native_pixel(255, 120), color(15));
        // fan line endpoints: colour 1 + i % 15
        assert_eq!(vdp.native_pixel(8, 48), color(1));
        assert_eq!(vdp.native_pixel(24, 48), color(2));
        assert_eq!(vdp.native_pixel(232, 48), color(15));
        assert_eq!(vdp.native_pixel(248, 48), color(1));
        // corner dots
        for (x, y) in [(10, 50), (245, 50), (10, 200), (245, 200)] {
            assert_eq!(vdp.native_pixel(x, y), color(15), "dot ({x},{y})");
        }
        // sprites (ball centre) and a transparent sprite corner
        assert_eq!(vdp.native_pixel(68, 108), color(15));
        assert_eq!(vdp.native_pixel(188, 108), color(8));
        assert_eq!(vdp.native_pixel(180, 100), color(0));
        assert_eq!(vdp.native_pixel(183, 100), color(0));
        assert_eq!(vdp.native_pixel(185, 100), color(8));

        write_png(
            &repo_root,
            "v9938-demo.png",
            &machine.video_frame().unwrap(),
        );
    }

    // Serial console keeps working in graphics mode; T returns to text.
    machine.enqueue_input_text("T\r");
    assert!(run_until(&mut machine, 40_000_000, |output| {
        output.matches('>').count() >= 3
    }));
    let vdp = machine.v9938().unwrap();
    assert_eq!(vdp.screen_mode(), ScreenMode::Text2);
    assert_text_row(vdp, &font, 0, ">");
}

#[test]
fn tms9918a_text_console_renders_banner_and_scrolls() {
    let _guard = integration_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let repo_root = repo_root();
    build_bios(&repo_root, Some("TMS9918=1"));
    let font = std::fs::read(repo_root.join("bios/font.bin")).expect("read bios/font.bin");
    let mut machine = machine_for_rom(&repo_root, MachineModel::V2, VdpChip::Tms9918a);
    assert!(machine.v9938().is_none(), "only one VDP is fitted");

    assert!(machine.run_until_output_contains(">", 20_000_000));
    {
        let vdp = machine.tms9918a().expect("TMS9918A fitted");
        assert_eq!(vdp.screen_mode(), ScreenMode::Text1);
        assert_tms_text_row(vdp, &font, 0, "Mini 68008 BIOS");
        assert_tms_text_row(vdp, &font, 1, ">");
        let frame = machine.video_frame().expect("v2 frame");
        assert_eq!((frame.width, frame.height), (512, 192));
        write_png(&repo_root, "tms9918-text.png", &frame);
    }

    // 16 dump lines of 54 characters wrap onto two 40-column rows each,
    // scrolling the banner away.
    machine.enqueue_input_text("D 10000 0010\r");
    assert!(run_until(&mut machine, 60_000_000, |output| {
        output.contains("0100F0") && output.ends_with("\r\n>")
    }));
    let vdp = machine.tms9918a().unwrap();
    assert_tms_text_row(vdp, &font, 23, ">");
    assert_tms_text_row(vdp, &font, 21, "0100F0 ");
    assert!(!text_row_matches(
        |x, y| vdp.native_pixel(x, y),
        vdp.palette_rgb(TEXT_FG),
        vdp.palette_rgb(TEXT_BG),
        &font,
        0,
        "Mini",
    ));
    write_png(
        &repo_root,
        "tms9918-scroll.png",
        &machine.video_frame().unwrap(),
    );
}

#[test]
fn tms9918a_graphics_demo_draws_bitmap_and_sprites() {
    let _guard = integration_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let repo_root = repo_root();
    build_bios(&repo_root, Some("TMS9918=1"));
    let font = std::fs::read(repo_root.join("bios/font.bin")).expect("read bios/font.bin");
    let mut machine = machine_for_rom(&repo_root, MachineModel::V2, VdpChip::Tms9918a);
    assert!(machine.run_until_output_contains(">", 20_000_000));

    machine.enqueue_input_text("G\r");
    assert!(run_until(&mut machine, 80_000_000, |output| {
        output.matches('>').count() >= 2
    }));

    {
        let vdp = machine.tms9918a().unwrap();
        assert_eq!(vdp.screen_mode(), ScreenMode::Graphic2);
        let color = |index: u8| vdp.palette_rgb(index);
        let backdrop = color(1);

        // colour bars (colour 0 is transparent: shows the black backdrop)
        assert_eq!(vdp.native_pixel(8, 16), backdrop);
        for c in 1..16u8 {
            assert_eq!(
                vdp.native_pixel(usize::from(c) * 16 + 8, 16),
                color(c),
                "bar {c}"
            );
        }
        // frame outline
        assert_eq!(vdp.native_pixel(100, 40), color(15));
        assert_eq!(vdp.native_pixel(100, 191), color(15));
        assert_eq!(vdp.native_pixel(0, 120), color(15));
        assert_eq!(vdp.native_pixel(255, 120), color(15));
        // fan line endpoints: colour 2 + i % 14
        assert_eq!(vdp.native_pixel(24, 48), color(3));
        assert_eq!(vdp.native_pixel(40, 48), color(4));
        assert_eq!(vdp.native_pixel(232, 48), color(2));
        assert_eq!(vdp.native_pixel(248, 48), color(3));
        // corner dots
        for (x, y) in [(10, 50), (245, 50), (10, 180), (245, 180)] {
            assert_eq!(vdp.native_pixel(x, y), color(15), "dot ({x},{y})");
        }
        // sprites (ball centre)
        assert_eq!(vdp.native_pixel(68, 108), color(15));
        assert_eq!(vdp.native_pixel(188, 108), color(8));

        write_png(
            &repo_root,
            "tms9918-demo.png",
            &machine.video_frame().unwrap(),
        );
    }

    machine.enqueue_input_text("T\r");
    assert!(run_until(&mut machine, 40_000_000, |output| {
        output.matches('>').count() >= 3
    }));
    let vdp = machine.tms9918a().unwrap();
    assert_eq!(vdp.screen_mode(), ScreenMode::Text1);
    assert_tms_text_row(vdp, &font, 0, ">");
}

#[test]
fn bounce_demo_runs_on_both_vdps() {
    let _guard = integration_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let repo_root = repo_root();
    run_make(&repo_root, &["-B", "-C", "demos"]);
    let segments = rom::load_srec_segments(&repo_root.join("demos/bounce.s28")).expect("demo");

    for (flag, chip, attributes, hidden) in [
        ("V9938=1", VdpChip::V9938, 0x7600usize, 217u8),
        ("TMS9918=1", VdpChip::Tms9918a, 0x3b00usize, 209u8),
    ] {
        build_bios(&repo_root, Some(flag));
        let mut machine = machine_for_rom(&repo_root, MachineModel::V2, chip);
        for segment in &segments {
            machine.load_ram(segment.address, &segment.data).unwrap();
        }
        machine.type_at_prompt("J 10000\r");
        assert!(
            machine.run_until_output_contains("Bounce demo", 20_000_000),
            "{chip:?}"
        );
        machine.run_for_cycles(20_000_000);

        let vram = |machine: &Machine| -> Vec<u8> {
            match chip {
                VdpChip::V9938 => machine.v9938().unwrap().vram().to_vec(),
                VdpChip::Tms9918a => machine.tms9918a().unwrap().vram().to_vec(),
            }
        };
        let before = vram(&machine);
        for ball in 0..3 {
            assert_ne!(
                before[attributes + ball * 4],
                hidden,
                "{chip:?} ball {ball} shown"
            );
        }

        let frame = machine.video_frame().unwrap();
        let (sky, ground) = match chip {
            VdpChip::V9938 => {
                let vdp = machine.v9938().unwrap();
                assert_eq!(vdp.screen_mode(), ScreenMode::Graphic4);
                (
                    vdp.native_pixel(100, 5) == vdp.palette_rgb(4),
                    vdp.native_pixel(5, 170) == vdp.palette_rgb(12),
                )
            }
            VdpChip::Tms9918a => {
                let vdp = machine.tms9918a().unwrap();
                assert_eq!(vdp.screen_mode(), ScreenMode::Graphic2);
                (
                    vdp.native_pixel(100, 5) == vdp.palette_rgb(4),
                    vdp.native_pixel(5, 170) == vdp.palette_rgb(12),
                )
            }
        };
        assert!(sky && ground, "{chip:?} scene drawn");
        let name = match chip {
            VdpChip::V9938 => "bounce-v9938.png",
            VdpChip::Tms9918a => "bounce-tms9918.png",
        };
        write_png(&repo_root, name, &frame);

        machine.run_for_cycles(2_000_000);
        let after = vram(&machine);
        assert!(
            (0..3).any(
                |ball| before[attributes + ball * 4..attributes + ball * 4 + 2]
                    != after[attributes + ball * 4..attributes + ball * 4 + 2]
            ),
            "{chip:?} balls move"
        );
    }
}

fn run_until(machine: &mut Machine, max_cycles: u64, done: impl Fn(&str) -> bool) -> bool {
    let deadline = machine.total_cycles() + max_cycles;
    while machine.total_cycles() < deadline {
        if done(&machine.output_as_string()) {
            return true;
        }
        machine.run_for_cycles(50_000);
    }
    done(&machine.output_as_string())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn build_bios(repo_root: &Path, vdp_flag: Option<&str>) {
    let mut args = vec!["-B", "-C", "bios", "FORMAT=binary"];
    args.extend(vdp_flag);
    run_make(repo_root, &args);
}

/// Run make with the cross toolchain, falling back to the Docker image.
fn run_make(repo_root: &Path, args: &[&str]) {
    let status = if cross_toolchain_available() {
        Command::new("make")
            .current_dir(repo_root)
            .args(args)
            .status()
            .expect("run make")
    } else {
        Command::new("docker")
            .current_dir(repo_root)
            .args(["compose", "run", "--rm", "dev", "make"])
            .args(args)
            .status()
            .expect("run make in docker")
    };
    assert!(status.success(), "make {args:?} failed");
}

fn cross_toolchain_available() -> bool {
    match Command::new("m68k-unknown-elf-as")
        .arg("--version")
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

fn machine_for_rom(repo_root: &Path, machine_model: MachineModel, vdp_chip: VdpChip) -> Machine {
    let rom_path = repo_root.join("bios/bios.bin");
    let rom_image = rom::load_rom(&rom_path).expect("load BIOS image");

    let mut config = MachineConfig::new(rom_image);
    config.ram_size = 32 * 1024;
    config.machine_model = machine_model;
    config.vdp_chip = vdp_chip;
    Machine::new(config)
}

/// Check that TEXT2 row `row` starts with `text`, rendered from the BIOS
/// font in the console colours (6-pixel cells: bits 7..2 of each row).
fn assert_text_row(vdp: &V9938, font: &[u8], row: usize, text: &str) {
    assert_text_row_with(
        |x, y| vdp.native_pixel(x, y),
        vdp.palette_rgb(TEXT_FG),
        vdp.palette_rgb(TEXT_BG),
        font,
        row,
        text,
    );
}

fn assert_tms_text_row(vdp: &Tms9918a, font: &[u8], row: usize, text: &str) {
    assert_text_row_with(
        |x, y| vdp.native_pixel(x, y),
        vdp.palette_rgb(TEXT_FG),
        vdp.palette_rgb(TEXT_BG),
        font,
        row,
        text,
    );
}

fn text_row_matches(
    pixel: impl Fn(usize, usize) -> u32,
    fg: u32,
    bg: u32,
    font: &[u8],
    row: usize,
    text: &str,
) -> bool {
    text.bytes().enumerate().all(|(column, character)| {
        (0..8).all(|glyph_row| {
            let pattern = font[usize::from(character) * 8 + glyph_row];
            (0..6).all(|bit| {
                let expected = if pattern & (0x80 >> bit) != 0 { fg } else { bg };
                pixel(column * 6 + bit, row * 8 + glyph_row) == expected
            })
        })
    })
}

/// Check that a 6-pixel-cell text row starts with `text`, rendered from the
/// BIOS font in the console colours.
fn assert_text_row_with(
    pixel: impl Fn(usize, usize) -> u32,
    fg: u32,
    bg: u32,
    font: &[u8],
    row: usize,
    text: &str,
) {
    for (column, character) in text.bytes().enumerate() {
        for glyph_row in 0..8 {
            let pattern = font[usize::from(character) * 8 + glyph_row];
            for bit in 0..6 {
                let expected = if pattern & (0x80 >> bit) != 0 { fg } else { bg };
                let actual = pixel(column * 6 + bit, row * 8 + glyph_row);
                assert_eq!(
                    actual,
                    expected,
                    "row {row} column {column} ({:?}) glyph row {glyph_row} bit {bit}",
                    char::from(character)
                );
            }
        }
    }
}

/// Snapshot for visual inspection: the 512x212 canvas with lines doubled.
fn write_png(repo_root: &Path, name: &str, frame: &VideoFrame) {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.join("target"));
    std::fs::create_dir_all(&dir).expect("create target dir");
    let path = dir.join(name);
    let file = File::create(&path).expect("create png");
    let mut encoder = png::Encoder::new(
        BufWriter::new(file),
        frame.width as u32,
        (frame.height * 2) as u32,
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    let mut data = Vec::with_capacity(frame.width * frame.height * 6);
    for y in 0..frame.height {
        let mut row = Vec::with_capacity(frame.width * 3);
        for x in 0..frame.width {
            let pixel = frame.pixel(x, y);
            row.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
        data.extend_from_slice(&row);
        data.extend_from_slice(&row);
    }
    writer.write_image_data(&data).expect("png data");
}
