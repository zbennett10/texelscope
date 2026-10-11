use std::collections::BTreeSet;

use crate::model::{
    Binding, Capture, FORMAT_VERSION, FragmentInputs, Resource, Texture, TextureData,
    TextureDimension,
};

/// Lists every problem with a capture. An empty list means it is valid.
pub fn validate(capture: &Capture) -> Vec<String> {
    let mut problems = Vec::new();
    if capture.version != FORMAT_VERSION {
        problems.push(format!(
            "version is {}, but this build reads version {FORMAT_VERSION}",
            capture.version
        ));
    }
    if capture.shader.entry_point.is_empty() {
        problems.push("shader.entryPoint is empty".to_string());
    }
    let target = capture.target;
    if target.width == 0 || target.height == 0 {
        problems.push(format!(
            "target is {}x{}, but both sides must be at least 1",
            target.width, target.height
        ));
    }
    if let FragmentInputs::Fullscreen { locations } = &capture.inputs {
        let mut seen = BTreeSet::new();
        for input in locations {
            if !seen.insert(input.location) {
                problems.push(format!("inputs list location {} twice", input.location));
            }
            let len = input.corners[0].len();
            if !(1..=4).contains(&len) || input.corners.iter().any(|c| c.len() != len) {
                problems.push(format!(
                    "location {} needs four corners with the same 1 to 4 components",
                    input.location
                ));
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut named = BTreeSet::new();
    for binding in &capture.bindings {
        // A WebGL binding goes by its GLSL name, and a sampler uniform gives a texture and a
        // sampler under the same name.
        if let Some(name) = &binding.name {
            let kind = std::mem::discriminant(&binding.resource);
            if !named.insert((name.clone(), format!("{kind:?}"))) {
                problems.push(format!("{name} appears twice"));
            }
        } else if !seen.insert((binding.group, binding.binding)) {
            problems.push(format!(
                "group {} binding {} appears twice",
                binding.group, binding.binding
            ));
        }
        if let Resource::Texture(texture) = &binding.resource {
            check_texture(binding, texture, &mut problems);
        }
    }
    problems
}

fn check_texture(binding: &Binding, texture: &Texture, problems: &mut Vec<String>) {
    let at = format!("group {} binding {}", binding.group, binding.binding);
    let [width, height, depth] = texture.size;
    if width == 0 || height == 0 || depth == 0 {
        problems.push(format!("{at} has an empty size {width}x{height}x{depth}"));
        return;
    }
    if texture.dimension == TextureDimension::D1 && (height != 1 || depth != 1) {
        problems.push(format!("{at} is 1d, so its height and depth must be 1"));
    }
    let largest = match texture.dimension {
        TextureDimension::D3 => width.max(height).max(depth),
        _ => width.max(height),
    };
    let max_levels = u32::BITS - largest.leading_zeros();
    if texture.mip_level_count == 0 || texture.mip_level_count > max_levels {
        problems.push(format!(
            "{at} has mipLevelCount {}, but its size allows 1 to {max_levels}",
            texture.mip_level_count
        ));
        return;
    }
    if let TextureData::Inline(values) = &texture.data {
        let expected = texel_count(texture) * u64::from(texture.format.components());
        if values.len() as u64 != expected {
            problems.push(format!(
                "{at} has {} inline values, but its size and format need {expected}",
                values.len()
            ));
        }
    }
}

/// Texels across all mip levels. The caller has checked the level count against the size.
fn texel_count(texture: &Texture) -> u64 {
    let [width, height, depth] = texture.size;
    (0..texture.mip_level_count)
        .map(|level| {
            let shrink = |v: u32| u64::from((v >> level).max(1));
            let layers = match texture.dimension {
                TextureDimension::D3 => shrink(depth),
                _ => u64::from(depth),
            };
            shrink(width) * shrink(height) * layers
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use crate::{CaptureError, parse};

    const BASE: &str = r#"{
        "version": 0,
        "shader": { "language": "wgsl", "source": { "inline": "" }, "entryPoint": "fs_main" },
        "target": { "width": 4, "height": 4, "format": "rgba8unorm" },
        "bindings": [BINDINGS]
    }"#;

    fn problems(bindings: &str) -> Vec<String> {
        match parse(&BASE.replace("BINDINGS", bindings)) {
            Ok(_) => Vec::new(),
            Err(CaptureError::Invalid(problems)) => problems,
            Err(other) => panic!("expected validation problems, got {other}"),
        }
    }

    fn texture(size: &str, mips: u32, values: usize) -> String {
        let inline = vec!["0"; values].join(",");
        format!(
            r#"{{ "group": 0, "binding": 0, "resource": {{ "kind": "texture", "dimension": "2d",
            "format": "r32float", "size": {size}, "mipLevelCount": {mips}, "data": {{ "inline": [{inline}] }} }} }}"#
        )
    }

    #[test]
    fn accepts_inline_data_that_covers_every_mip_level() {
        assert!(problems(&texture("[4, 2, 1]", 3, 8 + 2 + 1)).is_empty());
    }

    #[test]
    fn rejects_inline_data_of_the_wrong_length() {
        let found = problems(&texture("[4, 4, 1]", 1, 15));
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("need 16"), "{found:?}");
    }

    #[test]
    fn rejects_more_mip_levels_than_the_size_allows() {
        let found = problems(&texture("[4, 4, 1]", 4, 21));
        assert!(found[0].contains("allows 1 to 3"), "{found:?}");
    }

    #[test]
    fn rejects_duplicate_bindings() {
        let sampler = r#"{ "group": 0, "binding": 1, "resource": { "kind": "sampler" } }"#;
        let found = problems(&format!("{sampler}, {sampler}"));
        assert_eq!(found, ["group 0 binding 1 appears twice"]);
    }

    #[test]
    fn rejects_other_versions() {
        let json = BASE
            .replace("BINDINGS", "")
            .replace("\"version\": 0", "\"version\": 1");
        let Err(CaptureError::Invalid(found)) = parse(&json) else {
            panic!("version 1 should not parse");
        };
        assert!(found[0].starts_with("version is 1"), "{found:?}");
    }
}
