use mini68k_emu::machine::{
    Machine, MachineConfig, MachineModel, VdpChip, VideoOutput, DEFAULT_CPU_CLOCK_HZ,
    DEFAULT_RAM_SIZE,
};
use mini68k_emu::rom;
use minifb::{Window, WindowOptions};
use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process;
/// The window shows the 512x212 canvas with each line doubled so the
/// picture keeps a 4:3-ish aspect.
const LINE_DOUBLE: usize = 2;

fn main() {
    if let Err(error) = try_main() {
        eprintln!("{error}");
        print_usage();
        process::exit(2);
    }
}

fn try_main() -> Result<(), Box<dyn Error>> {
    let args = parse_args(env::args().skip(1).collect())?;
    let rom_image = rom::load_rom(&args.rom)?;

    let mut config = MachineConfig::new(rom_image);
    config.ram_size = args.ram_size;
    config.trace = args.trace;
    config.echo_stdout = true;
    config.start_stdin_thread = true;
    config.machine_model = args.machine_model;
    config.video_output = args.video_output;
    config.cpu_hz = args.cpu_hz;
    config.vdp_irq_level = args.vdp_irq;
    config.vdp_chip = args.vdp_chip;

    let mut machine = Machine::new(config);
    for path in &args.loads {
        let segments = rom::load_srec_segments(path)
            .map_err(|error| format!("--load {}: {error}", path.display()))?;
        for segment in segments {
            machine
                .load_ram(segment.address, &segment.data)
                .map_err(|error| format!("--load {}: {error}", path.display()))?;
        }
    }
    if let Some(text) = &args.type_text {
        machine.type_at_prompt(&unescape(text));
    }
    match args.video_output {
        VideoOutput::Headless => {
            machine.run(args.max_cycles);
            Ok(())
        }
        VideoOutput::Window => {
            if args.machine_model != MachineModel::V2 {
                return Err("--video window requires --machine v2".into());
            }
            run_with_window(&mut machine, args.max_cycles, args.video_scale, args.cpu_hz)
        }
    }
}

#[derive(Debug)]
struct Args {
    rom: PathBuf,
    ram_size: usize,
    trace: bool,
    max_cycles: Option<u64>,
    machine_model: MachineModel,
    video_output: VideoOutput,
    video_scale: usize,
    cpu_hz: u64,
    vdp_irq: Option<u32>,
    vdp_chip: VdpChip,
    loads: Vec<PathBuf>,
    type_text: Option<String>,
}

fn parse_args(arguments: Vec<String>) -> Result<Args, Box<dyn Error>> {
    let mut rom = None;
    let mut ram_size = DEFAULT_RAM_SIZE;
    let mut trace = false;
    let mut max_cycles = None;
    let mut machine_model = MachineModel::V1;
    let mut video_output = VideoOutput::Headless;
    let mut video_scale = 2usize;
    let mut cpu_hz = DEFAULT_CPU_CLOCK_HZ;
    let mut vdp_irq = None;
    let mut vdp_chip = VdpChip::default();
    let mut loads = Vec::new();
    let mut type_text = None;

    let mut index = 0usize;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--rom" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--rom requires a path".to_string())?;
                rom = Some(PathBuf::from(value));
            }
            "--ram-size" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--ram-size requires a value".to_string())?;
                ram_size = parse_number(value)? as usize;
            }
            "--max-cycles" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--max-cycles requires a value".to_string())?;
                max_cycles = Some(parse_number(value)?);
            }
            "--machine" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--machine requires v1 or v2".to_string())?;
                machine_model = parse_machine_model(value)?;
            }
            "--video" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--video requires headless or window".to_string())?;
                video_output = parse_video_output(value)?;
            }
            "--video-scale" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--video-scale requires a value".to_string())?;
                video_scale = parse_number(value)? as usize;
                if video_scale == 0 {
                    return Err("--video-scale must be greater than zero".into());
                }
            }
            "--cpu-hz" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--cpu-hz requires a value".to_string())?;
                cpu_hz = parse_number(value)?;
                if cpu_hz == 0 {
                    return Err("--cpu-hz must be greater than zero".into());
                }
            }
            "--vdp-irq" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--vdp-irq requires a level 1-7".to_string())?;
                let level = parse_number(value)?;
                if !(1..=7).contains(&level) {
                    return Err("--vdp-irq level must be 1-7".into());
                }
                vdp_irq = Some(level as u32);
            }
            "--vdp" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--vdp requires v9938 or tms9918a".to_string())?;
                vdp_chip = match value.as_str() {
                    "v9938" => VdpChip::V9938,
                    "tms9918a" | "tms9918" => VdpChip::Tms9918a,
                    other => return Err(format!("unsupported VDP: {other}").into()),
                };
            }
            "--load" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--load requires an S-record file".to_string())?;
                loads.push(PathBuf::from(value));
            }
            "--type" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| "--type requires text".to_string())?;
                type_text = Some(value.clone());
            }
            "--trace" => trace = true,
            "--help" | "-h" => {
                print_usage();
                process::exit(0);
            }
            unknown => {
                return Err(format!("unrecognized argument: {unknown}").into());
            }
        }
        index += 1;
    }

    let rom = rom.ok_or_else(|| "--rom is required".to_string())?;
    Ok(Args {
        rom,
        ram_size,
        trace,
        max_cycles,
        machine_model,
        video_output,
        video_scale,
        cpu_hz,
        vdp_irq,
        vdp_chip,
        loads,
        type_text,
    })
}

fn parse_machine_model(text: &str) -> Result<MachineModel, Box<dyn Error>> {
    match text {
        "v1" => Ok(MachineModel::V1),
        "v2" => Ok(MachineModel::V2),
        _ => Err(format!("unsupported machine model: {text}").into()),
    }
}

fn parse_video_output(text: &str) -> Result<VideoOutput, Box<dyn Error>> {
    match text {
        "headless" => Ok(VideoOutput::Headless),
        "window" => Ok(VideoOutput::Window),
        _ => Err(format!("unsupported video mode: {text}").into()),
    }
}

/// Expand `\r`, `\n` and `\\` in `--type` text.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn parse_number(text: &str) -> Result<u64, Box<dyn Error>> {
    let value = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)?
    } else {
        text.parse()?
    };
    Ok(value)
}

fn run_with_window(
    machine: &mut Machine,
    max_cycles: Option<u64>,
    video_scale: usize,
    cpu_hz: u64,
) -> Result<(), Box<dyn Error>> {
    // One host frame per emulated 60 Hz frame keeps the CPU near real speed.
    let slice_cycles = (cpu_hz / 60).max(1);
    machine.boot();
    let initial_frame = machine
        .video_frame()
        .ok_or_else(|| "video output is not available for this machine".to_string())?;
    let (window_width, window_height) = (
        initial_frame.width.saturating_mul(video_scale),
        initial_frame
            .height
            .saturating_mul(video_scale * LINE_DOUBLE),
    );

    let mut window = Window::new(
        match machine.vdp().map(|vdp| vdp.chip()) {
            Some(VdpChip::Tms9918a) => "mini68k TMS9918A",
            _ => "mini68k V9938",
        },
        window_width,
        window_height,
        WindowOptions::default(),
    )
    .map_err(|error| {
        format!(
            "failed to create window: {error}. Windowed mode needs a host desktop session; if you launched this through Docker, rerun it on the host or use --video headless."
        )
    })?;
    window.set_target_fps(60);

    let mut remaining_cycles = max_cycles;
    while window.is_open() {
        let slice = remaining_cycles
            .map(|remaining| remaining.min(slice_cycles))
            .unwrap_or(slice_cycles);

        if slice == 0 {
            break;
        }

        machine.run_for_cycles(slice);
        if let Some(remaining) = remaining_cycles.as_mut() {
            *remaining = remaining.saturating_sub(slice);
        }

        if let Some(frame) = machine.video_frame() {
            let scaled = scale_frame(
                &frame.pixels_rgba,
                frame.width,
                frame.height,
                video_scale,
                video_scale * LINE_DOUBLE,
            );
            window.update_with_buffer(&scaled, window_width, window_height)?;
        } else {
            window.update();
        }
    }

    Ok(())
}

fn scale_frame(
    source: &[u32],
    width: usize,
    height: usize,
    scale_x: usize,
    scale_y: usize,
) -> Vec<u32> {
    let scaled_width = width * scale_x;
    let mut scaled = vec![0u32; scaled_width * height * scale_y];

    for y in 0..height {
        let row = &mut scaled[(y * scale_y) * scaled_width..(y * scale_y + 1) * scaled_width];
        for x in 0..width {
            let color = source[(y * width) + x] & 0x00ff_ffff;
            row[x * scale_x..(x + 1) * scale_x].fill(color);
        }
        for dy in 1..scale_y {
            let (done, rest) = scaled.split_at_mut((y * scale_y + dy) * scaled_width);
            rest[..scaled_width].copy_from_slice(
                &done[(y * scale_y) * scaled_width..(y * scale_y + 1) * scaled_width],
            );
        }
    }

    scaled
}

fn print_usage() {
    eprintln!(
        "usage: mini68k-emu --rom <path> [--machine v1|v2] [--video headless|window] [--video-scale <n>] [--vdp v9938|tms9918a] [--cpu-hz <hz>] [--vdp-irq <level>] [--load <file.s28>]... [--type <keys>] [--ram-size <bytes>] [--max-cycles <n>] [--trace]"
    );
}
