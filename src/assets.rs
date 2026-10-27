//! Icons compiled into the binary; GPUI renders them as masks in the text color.
pub struct Assets;

/// `icons/<name>` for each file under `assets/icons`.
macro_rules! icons {
    ($($name:literal,)*) => {
        &[$((
            concat!("icons/", $name),
            include_bytes!(concat!("../assets/icons/", $name)) as &[u8],
        ),)*]
    };
}

const ICONS: &[(&str, &[u8])] = icons!(
    "pin.svg",
    "corner.svg",
    "search.svg",
    "gear.svg",
    "wrench.svg",
    "folder.svg",
    "file.svg",
    "diff.svg",
    "chevron-right.svg",
    "chevron-down.svg",
    "terminal.svg",
    "zap.svg",
    "circle-x.svg",
    "triangle-alert.svg",
    "lang/rust.svg",
    "lang/python.svg",
    "lang/markdown.svg",
    "lang/javascript.svg",
    "lang/typescript.svg",
    "lang/react.svg",
    "lang/go.svg",
    "lang/json.svg",
    "lang/toml.svg",
    "lang/yaml.svg",
    "lang/html5.svg",
    "lang/css3.svg",
    "lang/c.svg",
    "lang/cplusplus.svg",
    "lang/ruby.svg",
    "lang/swift.svg",
    "lang/lua.svg",
    "lang/gnubash.svg",
    "lang/docker.svg",
    "lang/git.svg",
);

impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| (*bytes).into()))
    }

    fn list(&self, _: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        Ok(ICONS.iter().map(|(name, _)| (*name).into()).collect())
    }
}
