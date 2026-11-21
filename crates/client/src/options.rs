use crate::scenarios::Scenario;
use primal_streaming::StreamConfig;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mesher {
    Binary,
    Faces,
    Greedy,
}

impl Mesher {
    pub fn name(self) -> &'static str {
        match self {
            Self::Faces => "faces",
            Self::Binary => "binary",
            Self::Greedy => "greedy",
        }
    }
}

#[derive(Debug)]
pub struct Options {
    pub compare: bool,
    pub playtest: bool,
    pub compare_drift: bool,
    pub connect: Option<String>,
    pub identity: PathBuf,
    pub net_frames: Option<usize>,
    pub net_report: Option<PathBuf>,
    pub direct_solo: bool,
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
    pub scenario: Scenario,
    pub valley_only: bool,
    pub frustum: bool,
    pub stream: StreamConfig,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            compare: false,
            playtest: false,
            compare_drift: false,
            connect: None,
            identity: default_save_path().with_file_name("client.identity"),
            net_frames: None,
            net_report: None,
            direct_solo: false,
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
            scenario: Scenario::Valley,
            valley_only: false,
            frustum: true,
            stream: StreamConfig {
                load_radius: 8,
                render_radius: 7,
                ..StreamConfig::default()
            },
        };
        let mut explicit_world = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--test-input" => options.playtest = true,
                "--compare" => options.compare = true,
                "--compare-route" => {
                    options.compare_drift = match value(&mut args, &arg)?.as_str() {
                        "fixed" => false,
                        "drift" => true,
                        _ => return Err("--compare-route attend fixed ou drift".into()),
                    }
                }
                "--connect" => options.connect = Some(value(&mut args, &arg)?),
                "--identity" => options.identity = PathBuf::from(value(&mut args, &arg)?),
                "--net-frames" => options.net_frames = Some(number(&mut args, &arg)?),
                "--net-report" => options.net_report = Some(PathBuf::from(value(&mut args, &arg)?)),
                "--edit-route" => options.edit_route = true,
                "--profile" => options.profile = Some(PathBuf::from(value(&mut args, &arg)?)),
                "--no-frustum" => options.frustum = false,
                "--valley-only" => options.valley_only = true,
                "--smoke" => options.smoke = true,
                "--scenario" => {
                    options.scenario = value(&mut args, &arg)?.parse()?;
                    options.smoke = true;
                }
                "--load-radius" => options.stream.load_radius = number(&mut args, &arg)?,
                "--render-radius" => options.stream.render_radius = number(&mut args, &arg)?,
                "--simulation-radius" => {
                    options.stream.simulation_radius = number(&mut args, &arg)?
                }
                "--preload-radius" => options.stream.preload_radius = number(&mut args, &arg)?,
                "--max-chunks" => options.stream.max_chunks = number(&mut args, &arg)?,
                "--workers" => options.stream.workers = number(&mut args, &arg)?,
                "--max-jobs" => options.stream.max_jobs = number(&mut args, &arg)?,
                "--uploads" => options.stream.uploads_per_tick = number(&mut args, &arg)?,
                "--resident-mib" => options.stream.resident_bytes = mib(&mut args, &arg)?,
                "--prepared-mib" => options.stream.prepared_bytes = mib(&mut args, &arg)?,
                "--mesh-cache-mib" => options.stream.mesh_cache_bytes = mib(&mut args, &arg)?,
                "--main-budget-us" => {
                    options.stream.main_budget = Duration::from_micros(number(&mut args, &arg)?)
                }
                "--world" => {
                    options.world = PathBuf::from(value(&mut args, &arg)?);
                    explicit_world = true;
                    options.direct_solo = true;
                }
                "--capture" => options.capture = Some(PathBuf::from(value(&mut args, &arg)?)),
                "--metrics" => options.metrics = Some(PathBuf::from(value(&mut args, &arg)?)),
                "--mesher" => {
                    options.mesher = match value(&mut args, &arg)?.as_str() {
                        "faces" => Mesher::Faces,
                        "binary" => Mesher::Binary,
                        "greedy" => Mesher::Greedy,
                        _ => return Err("--mesher attend faces, greedy ou binary".into()),
                    };
                }
                "--tick-rate" => {
                    options.tick_rate = value(&mut args, &arg)?
                        .parse()
                        .map_err(|_| "--tick-rate attend un entier")?;
                    if !(10..=120).contains(&options.tick_rate) {
                        return Err("--tick-rate doit être compris entre 10 et 120".into());
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
        options
            .stream
            .validate()
            .map_err(|error| error.to_string())?;
        if options.connect.is_none() && options.tick_rate < 30 {
            return Err("Le solo exige au moins 30 ticks/s".into());
        }
        if options.net_frames == Some(0) {
            return Err("--net-frames exige au moins une image".into());
        }
        if options.compare_drift && !options.compare {
            return Err("--compare-route exige --compare".into());
        }
        if options.connect.is_some() && options.smoke {
            return Err("--connect et --smoke sont deux parcours distincts".into());
        }
        if options.connect.is_none()
            && (options.net_frames.is_some() || options.net_report.is_some())
        {
            return Err("--net-frames et --net-report exigent --connect".into());
        }
        if !options.frustum && !options.smoke {
            return Err("--no-frustum exige --smoke".into());
        }
        if options.valley_only && (!options.smoke || options.scenario != Scenario::Valley) {
            return Err("--valley-only exige le smoke valley".into());
        }
        if options.stream.chunk_limit() < 8 {
            return Err("Le client exige huit chunks pour la colonne de départ".into());
        }
        if options.edit_route && options.scenario != Scenario::Valley {
            return Err("--edit-route utilise le parcours de comparaison valley".into());
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
            options.world = std::env::temp_dir()
                .join(format!("primal-voxel-smoke-{}-{nonce}", std::process::id()))
                .join("world.pvx");
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
        let regions = primal_storage::RegionStore::directory(&options.world);
        let mut lock = options.world.as_os_str().to_owned();
        lock.push(".lock");
        for path in [
            &options.profile,
            &options.metrics,
            &options.capture,
            &options.net_report,
        ]
        .into_iter()
        .flatten()
        {
            if path.starts_with(&regions) || path.as_os_str() == lock {
                return Err("La sortie demandée appartient à la sauvegarde régionale".into());
            }
            if path == &options.identity || path == &options.world {
                return Err(
                    "La sortie demandée doit être distincte du monde et de l'identité".into(),
                );
            }
        }
        Ok(options)
    }
}

fn value(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("Valeur manquante pour {name}"))
}

fn number<T: std::str::FromStr>(
    args: &mut impl Iterator<Item = String>,
    name: &str,
) -> Result<T, String> {
    value(args, name)?
        .parse()
        .map_err(|_| format!("{name} attend un entier"))
}

fn mib(args: &mut impl Iterator<Item = String>, name: &str) -> Result<usize, String> {
    number::<usize>(args, name)?
        .checked_mul(1 << 20)
        .ok_or_else(|| format!("{name} dépasse la taille adressable"))
}

fn default_save_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("saves"));
    base.join("PrimalVoxel").join("world.pvx")
}

pub fn help() -> &'static str {
    "Primal Voxel\nSans argument : menu Solo / Héberger / Rejoindre\n--connect IP:PORT --identity CHEMIN --net-frames N --net-report RAPPORT.json\n--world CHEMIN --resolution 1280x720 --mesher faces|greedy|binary --tick-rate 60\n--smoke --frames 480 --capture IMAGE.png --metrics MESURES.json --profile ETAPES.csv --edit-route --valley-only --no-frustum\n--scenario valley|region512|region1024|traverse|teleport|edits\n--load-radius N --render-radius N --simulation-radius N --preload-radius N (en chunks)\n--max-chunks N --resident-mib N --prepared-mib N --mesh-cache-mib N\n--workers N --max-jobs N --uploads N --main-budget-us N"
}

#[cfg(test)]
mod tests {
    #[test]
    fn comparison_accepts_deterministic_local_and_remote_routes() {
        assert!(parse(&["--compare"]).is_ok());
        assert!(parse(&["--compare-route", "drift"]).is_err());
        assert!(
            parse(&[
                "--compare",
                "--compare-route",
                "drift",
                "--connect",
                "127.0.0.1:47620"
            ])
            .is_ok()
        );
        assert!(parse(&["--compare", "--compare-route", "random"]).is_err());
    }
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

    #[test]
    fn multiplayer_route_keeps_identity_and_outputs_distinct() {
        let options = parse(&[
            "--connect",
            "127.0.0.1:47620",
            "--identity",
            "a.identity",
            "--net-frames",
            "720",
            "--net-report",
            "a.json",
        ])
        .unwrap();
        assert_eq!(options.net_frames, Some(720));
        assert_eq!(options.identity, PathBuf::from("a.identity"));
        assert_eq!(options.connect.as_deref(), Some("127.0.0.1:47620"));
        assert!(parse(&["--net-frames", "720"]).is_err());
        assert!(parse(&["--connect", "127.0.0.1:47620", "--net-frames", "0"]).is_err());
        assert!(parse(&["--connect", "127.0.0.1:47620", "--smoke"]).is_err());
        assert!(
            parse(&[
                "--connect",
                "127.0.0.1:47620",
                "--identity",
                "a",
                "--net-report",
                "a"
            ])
            .is_err()
        );
    }

    #[test]
    fn stress_runs_use_new_worlds_and_apply_budgets() {
        let a = parse(&[
            "--scenario",
            "teleport",
            "--render-radius",
            "2",
            "--load-radius",
            "3",
            "--workers",
            "2",
            "--uploads",
            "1",
            "--resident-mib",
            "8",
        ])
        .unwrap();
        assert!(a.smoke && a.temporary_world);
        assert_eq!(a.scenario, Scenario::Teleport);
        assert_eq!(a.stream.resident_bytes, 8 << 20);
        assert_eq!((a.stream.workers, a.stream.uploads_per_tick), (2, 1));
        assert_ne!(a.world, default_save_path());
        assert!(parse(&["--scenario", "teleport", "--edit-route"]).is_err());
        assert!(parse(&["--scenario", "oops"]).is_err());
        assert!(
            parse(&[
                "--smoke",
                "--world",
                "save",
                "--metrics",
                "save.regions/world.pvxd"
            ])
            .is_err()
        );
    }
}
