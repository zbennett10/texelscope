//! The `texelscope` command.

mod render;
mod trace_text;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use texelscope_capture::{Language, LoadedCapture, Resource};
use texelscope_interp::{CaptureData, Interpreter};
use texelscope_ir::{Override, ShaderModule, Stage};

#[derive(Parser)]
#[command(
    name = "texelscope",
    version,
    about = "Explains why a shader pixel has its color"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse and validate a WGSL shader, then list its entry points and named values.
    Check {
        shader: PathBuf,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Translate a WGSL shader to another language.
    Emit {
        shader: PathBuf,
        #[arg(long, value_enum, default_value_t = EmitLanguage::GlslEs)]
        to: EmitLanguage,
        /// The entry point to emit GLSL for. Defaults to the first fragment entry point.
        #[arg(long)]
        entry: Option<String>,
    },
    /// Validate a capture file and summarize it.
    Capture { capture: PathBuf },
    /// Trace one pixel of a capture, showing each named value, decision and assignment.
    Trace {
        capture: PathBuf,
        /// The pixel as x,y.
        #[arg(long, value_parser = parse_pixel)]
        pixel: [u32; 2],
        /// Show every expression, not only named values.
        #[arg(long)]
        all: bool,
        /// Print the full trace as JSON.
        #[arg(long)]
        json: bool,
        /// A JSON file with an array of overrides to apply.
        #[arg(long)]
        overrides: Option<PathBuf>,
    },
    /// Explain why one pixel of a capture has its color.
    Explain {
        capture: PathBuf,
        /// The pixel as x,y.
        #[arg(long, value_parser = parse_pixel)]
        pixel: [u32; 2],
        /// Print the explanation as JSON.
        #[arg(long)]
        json: bool,
        /// A JSON file with an array of overrides to apply.
        #[arg(long)]
        overrides: Option<PathBuf>,
    },
    /// Shade every pixel of a capture on the CPU and write a PNG.
    Render {
        capture: PathBuf,
        /// Where to write the image.
        #[arg(short, long)]
        output: PathBuf,
        /// A JSON file with an array of overrides to apply.
        #[arg(long)]
        overrides: Option<PathBuf>,
    },
    /// Print the JSON Schema of the capture format.
    Schema,
}

#[derive(Clone, Copy, ValueEnum)]
enum EmitLanguage {
    Wgsl,
    GlslEs,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Check { shader, json } => check(&shader, json),
        Command::Emit { shader, to, entry } => emit(&shader, to, entry.as_deref()),
        Command::Capture { capture } => summarize(&capture),
        Command::Trace {
            capture,
            pixel,
            all,
            json,
            overrides,
        } => trace(&capture, pixel, all, json, overrides.as_deref()),
        Command::Explain {
            capture,
            pixel,
            json,
            overrides,
        } => explain(&capture, pixel, json, overrides.as_deref()),
        Command::Render {
            capture,
            output,
            overrides,
        } => {
            let interp = interpreter(&capture)?;
            let overrides = load_overrides(overrides.as_deref())?;
            render::write_png(&interp, &overrides, &output)?;
            println!("wrote {}", output.display());
            Ok(())
        }
        Command::Schema => {
            let schema = schemars::schema_for!(texelscope_capture::Capture);
            println!("{}", serde_json::to_string_pretty(&schema)?);
            Ok(())
        }
    }
}

fn check(path: &Path, json: bool) -> Result<()> {
    let module = load_shader(path)?;
    let entry_points = module.entry_points();
    let named = module.named_values();
    if json {
        let report = serde_json::json!({ "entryPoints": entry_points, "namedValues": named });
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!("{} is valid WGSL\n\nentry points", path.display());
    for ep in &entry_points {
        println!("  {:<14} {}", ep.name, ep.stage.as_str());
    }
    println!("\nnamed values");
    for value in &named {
        let line = value
            .at
            .map(|at| format!("line {}", at.line))
            .unwrap_or_default();
        let kind = name_of(&value.kind);
        println!(
            "  {:<14} {kind:<8} {:<14} {line}",
            value.function_name, value.name
        );
    }
    Ok(())
}

fn emit(path: &Path, to: EmitLanguage, entry: Option<&str>) -> Result<()> {
    let module = load_shader(path)?;
    let code = match to {
        EmitLanguage::Wgsl => texelscope_rewrite::emit_wgsl(&module)?,
        EmitLanguage::GlslEs => {
            let entry = match entry {
                Some(name) => name.to_string(),
                None => module
                    .entry_points()
                    .into_iter()
                    .find(|ep| ep.stage == Stage::Fragment)
                    .map(|ep| ep.name)
                    .context("the shader has no fragment entry point, so pass --entry")?,
            };
            texelscope_rewrite::emit_glsl_es(&module, &entry)?
        }
    };
    print!("{code}");
    Ok(())
}

fn summarize(path: &Path) -> Result<()> {
    let (loaded, _) = load_capture(path)?;
    let capture = &loaded.capture;
    println!("{} is a valid capture", path.display());
    if let Some(label) = &capture.label {
        println!("  label     {label}");
    }
    let shader = &capture.shader;
    println!(
        "  shader    {} {}",
        name_of(&shader.language),
        shader.entry_point
    );
    let target = capture.target;
    println!(
        "  target    {}x{} {}",
        target.width,
        target.height,
        name_of(&target.format)
    );
    println!("  inputs    {}", tag_of(&capture.inputs));
    for binding in &capture.bindings {
        let detail = match &binding.resource {
            Resource::Texture(t) => {
                let [w, h, d] = t.size;
                format!(
                    " {} {w}x{h}x{d} {}",
                    name_of(&t.dimension),
                    name_of(&t.format)
                )
            }
            _ => String::new(),
        };
        let label = binding
            .label
            .as_deref()
            .map(|l| format!(" ({l})"))
            .unwrap_or_default();
        println!(
            "  binding   {}.{} {}{detail}{label}",
            binding.group,
            binding.binding,
            tag_of(&binding.resource)
        );
    }
    Ok(())
}

fn trace(
    path: &Path,
    pixel: [u32; 2],
    all: bool,
    json: bool,
    overrides: Option<&Path>,
) -> Result<()> {
    let interp = interpreter(path)?;
    let trace = interp.trace_pixel(pixel, &load_overrides(overrides)?)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&trace)?);
    } else {
        print!("{}", trace_text::render(interp.module(), &trace, all));
    }
    Ok(())
}

fn explain(path: &Path, pixel: [u32; 2], json: bool, overrides: Option<&Path>) -> Result<()> {
    let interp = interpreter(path)?;
    let explanation =
        texelscope_explain::explain_pixel(&interp, pixel, &load_overrides(overrides)?)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&explanation)?);
    } else {
        print!("{}", texelscope_explain::render_text(&explanation));
    }
    Ok(())
}

/// Loads a capture and prepares an interpreter for it.
fn interpreter(path: &Path) -> Result<Interpreter> {
    let (loaded, module) = load_capture(path)?;
    let data = CaptureData::new(loaded.capture, Some(&loaded.base_dir))?;
    Ok(Interpreter::new(Arc::new(module), Arc::new(data))?)
}

fn load_overrides(path: Option<&Path>) -> Result<Vec<Override>> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("{} is not a JSON array of overrides", path.display()))
}

fn load_shader(path: &Path) -> Result<ShaderModule> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    ShaderModule::from_wgsl(source)
        .map_err(|e| anyhow!("{} is not valid WGSL\n\n{e}", path.display()))
}

/// Loads a capture and checks that its shader has the fragment entry point it names.
fn load_capture(path: &Path) -> Result<(LoadedCapture, ShaderModule)> {
    let loaded = texelscope_capture::load(path)?;
    if loaded.capture.shader.language != Language::Wgsl {
        bail!("only WGSL captures can be replayed so far");
    }
    let module = ShaderModule::from_wgsl(loaded.shader_source.clone())
        .map_err(|e| anyhow!("the capture's shader is not valid WGSL\n\n{e}"))?;
    let name = &loaded.capture.shader.entry_point;
    match module.entry_point(name) {
        Some(ep) if ep.stage == Stage::Fragment => Ok((loaded, module)),
        Some(_) => bail!("entry point {name:?} is not a fragment shader"),
        None => bail!("the shader has no entry point named {name:?}"),
    }
}

fn parse_pixel(text: &str) -> Result<[u32; 2], String> {
    let (x, y) = text.split_once(',').ok_or("expected x,y")?;
    let coordinate = |s: &str| s.trim().parse::<u32>().map_err(|e| format!("{s:?}: {e}"));
    Ok([coordinate(x)?, coordinate(y)?])
}

/// The name a unit enum variant has in JSON.
fn name_of<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::from("?"),
    }
}

/// The `kind` tag of an internally tagged value.
fn tag_of<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string))
        .unwrap_or_else(|| String::from("?"))
}
