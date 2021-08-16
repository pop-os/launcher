use std::path::PathBuf;

pub fn find(name: &'_ str) -> impl Iterator<Item = PathBuf> + '_ {
    crate::plugin_paths()
        .filter_map(|path| path.read_dir().ok())
        .flat_map(move |dir| {
            dir.filter_map(Result::ok).filter_map(move |entry| {
                if entry.file_name() == name {
                    let path = entry.path();
                    let config_path = path.join("config.ron");
                    if config_path.exists() {
                        return Some(config_path);
                    }
                }

                None
            })
        })
}
