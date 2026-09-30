pub fn expected_tag(version: &str, prefix: &str) -> String {
    format!("{prefix}{version}")
}

pub fn archive_root(name: &str, target: &str, version: &str) -> String {
    format!("{name}-{target}-v{version}")
}

pub fn archive_name(name: &str, target: &str, version: &str) -> String {
    format!("{}.tgz", archive_root(name, target, version))
}

pub fn binary_filename(binary: &str, target: &str) -> String {
    if target.split('-').any(|part| part == "windows") {
        format!("{binary}.exe")
    } else {
        binary.to_owned()
    }
}

pub fn binary_asset_name(binary: &str, target: &str, version: &str) -> String {
    binary_filename(&format!("{binary}-{target}-v{version}"), target)
}

pub fn binstall_metadata(prefix: &str) -> [(&'static str, String); 3] {
    let root = archive_root("{ name }", "{ target }", "{ version }");
    [
        (
            "pkg-url",
            format!("{{ repo }}/releases/download/{prefix}{{ version }}/{root}.tgz"),
        ),
        ("pkg-fmt", "tgz".to_owned()),
        ("bin-dir", format!("{root}/{{ bin }}{{ binary-ext }}")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_python_contract() {
        assert_eq!(expected_tag("0.3.1", ""), "0.3.1");
        assert_eq!(expected_tag("0.3.1", "release-"), "release-0.3.1");
        assert_eq!(
            archive_name("demo", "x86_64-pc-windows-gnu", "0.3.1"),
            "demo-x86_64-pc-windows-gnu-v0.3.1.tgz"
        );
        assert_eq!(binary_filename("demo", "x86_64-pc-windows-gnu"), "demo.exe");
        assert_eq!(binary_filename("demo", "x86_64-unknown-linux-musl"), "demo");
        assert_eq!(
            binary_asset_name("demo", "x86_64-pc-windows-gnu", "0.3.1"),
            "demo-x86_64-pc-windows-gnu-v0.3.1.exe"
        );
        let metadata = binstall_metadata("release-");
        assert_eq!(
            metadata[0].1,
            "{ repo }/releases/download/release-{ version }/{ name }-{ target }-v{ version }.tgz"
        );
        assert_eq!(
            metadata[2].1,
            "{ name }-{ target }-v{ version }/{ bin }{ binary-ext }"
        );
    }
}
