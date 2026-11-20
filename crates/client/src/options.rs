use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mesher {
    Faces,
    Greedy,
}

impl Mesher {
    pub fn name(self) -> &'static str {
        match self {
            Self::Faces => "faces",
            Self::Greedy => "greedy",
        }
    }
}

#[derive(Debug)]
pub struct Options {
    pub world: PathBuf,
    pub smoke: bool,
    pub profile: Option<PathBuf>,
    pub edit_route: bool,
    pub frames: usize,
    pub capture: Option<PathBuf>,
    pub metrics: Option<PathBuf>,
    pub width: i32,
    pub height: i32,
    pub temporary_world: bool,
    pub mesher: Mesher,
    pub tick_rate: u16,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            world: default_save_path(),
            smoke: false,
            frames: 480,
            profile: None,
            edit_route: false,
            capture: None,
            metrics: None,
            width: 1280,
            height: 720,
            temporary_world: false,
            mesher: Mesher::Greedy,
            tick_rate: 60,
        };
        let mut explicit_world = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--edit-route" => options.edit_route = true,
                "--profile" => options.profile = Some(PathBuf::from(value(&mut args, &arg)?)),
                "--smoke" => options.smoke = true,
                "--world" => {
                    options.world = PathBuf::from(value(&mut args, &arg)?);
                    explicit_world = true;
                }
                "--capture" => options.capture = Some(PathBuf::from(value(&mut args, &arg)?)),
                "--metrics" => options.metrics = Some(PathBuf::from(value(&mut args, &arg)?)),
                "--mesher" => {
                    options.mesher = match value(&mut args, &arg)?.as_str() {
                        "faces" => Mesher::Faces,
                        "greedy" => Mesher::Greedy,
                        _ => return Err("--mesher attend faces ou greedy".into()),
                    };
                }
                "--tick-rate" => {
                    options.tick_rate = value(&mut args, &arg)?
                        .parse()
                        .map_err(|_| "--tick-rate attend un entier")?;
                    if !(30..=120).contains(&options.tick_rate) {
                        return Err("--tick-rate doit être compris entre 30 et 120".into());
                    }
                }
                "--frames" => {
                    options.frames = value(&mut args, &arg)?
                        .parse::<usize>()
                        .map_err(|_| "--frames attend un entier".to_owned())?;
                    if options.frames < 180 {
                        return Err(
                            "--frames doit être au moins 180 (120 frames de chauffe)".into()
                        );
                    }
                }
                "--resolution" => {
                    let resolution = value(&mut args, &arg)?;
                    let (width, height) = resolution
                        .split_once('x')
                        .ok_or("--resolution attend LARGEURxHAUTEUR")?;
                    options.width = width.parse().map_err(|_| "Largeur invalide")?;
                    options.height = height.parse().map_err(|_| "Hauteur invalide")?;
                    if !(640..=3840).contains(&options.width)
                        || !(360..=2160).contains(&options.height)
                    {
                        return Err("Résolution hors limites 640x360 à 3840x2160".into());
                    }
                }
                "--help" | "-h" => return Err(help().into()),
                _ => return Err(format!("Argument inconnu {arg}\n{}", help())),
            }
        }
        if (options.edit_route || options.profile.is_some()) && !options.smoke {
            return Err("Les mesures exigent --smoke".into());
        }
        if options.profile.as_ref().is_some_and(|p| {
            p == &options.world
                || Some(p) == options.metrics.as_ref()
                || Some(p) == options.capture.as_ref()
        }) {
            return Err("Le profil exige un fichier distinct".into());
        }
        if options.smoke && options.tick_rate != 60 {
            return Err("Le parcours smoke exige --tick-rate 60".into());
        }
        if options.smoke && !explicit_world {
            options.temporary_world = true;
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos();
            options.world = std::env::temp_dir().join(format!(
                "primal-voxel-smoke-{}-{nonce}.pvx",
                std::process::id()
            ));
        }
        if options
            .capture
            .as_ref()
            .is_some_and(|path| path == &options.world)
            || options
                .metrics
                .as_ref()
                .is_some_and(|path| path == &options.world)
        {
            return Err(
                "Les fichiers de mesure et de capture doivent être distincts du monde".into(),
            );
        }
        Ok(options)
    }
}

fn value(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("Valeur manquante pour {name}"))
}

fn default_save_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("saves"));
    base.join("PrimalVoxel").join("world.pvx")
}

pub fn help() -> &'static str {
    "Primal Voxel\n--world CHEMIN --resolution 1280x720 --mesher faces|greedy --tick-rate 60\n--smoke --frames 480 --capture IMAGE.png --metrics MESURES.json --profile ETAPES.csv --edit-route"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn rejects_incomplete_or_invalid_commands() {
        assert!(parse(&["--world"]).is_err());
        assert!(parse(&["--resolution", "-1x720"]).is_err());
        assert!(parse(&["--frames", "0"]).is_err());
        assert!(parse(&["--unknown"]).is_err());
        assert!(parse(&["--world", "a.pvx", "--metrics", "a.pvx"]).is_err());
        assert!(parse(&["--tick-rate", "0"]).is_err());
        assert!(parse(&["--smoke", "--tick-rate", "30"]).is_err());
    }

    #[test]
    fn smoke_preserves_explicit_world_path() {
        let options = parse(&[
            "--smoke",
            "--world",
            "test.pvx",
            "--resolution",
            "1920x1080",
        ])
        .unwrap();
        assert_eq!(options.world, PathBuf::from("test.pvx"));
        assert_eq!((options.width, options.height), (1920, 1080));
    }
}
