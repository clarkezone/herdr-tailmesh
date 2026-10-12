//! Local presentation geometry, independent from outcome acknowledgements.
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub size: [f64; 2],
    pub position: Option<[i32; 2]>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preferences {
    pub normal: Bounds,
    pub compact: Bounds,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            normal: Bounds {
                size: [1440., 900.],
                position: None,
            },
            compact: Bounds {
                size: [520., 360.],
                position: None,
            },
        }
    }
}
impl Bounds {
    pub fn limited(mut self, compact: bool) -> Self {
        let (min, max) = if compact {
            ([360., 240.], [960., 720.])
        } else {
            ([640., 400.], [7680., 4320.])
        };
        for i in 0..2 {
            self.size[i] = if self.size[i].is_finite() {
                self.size[i].clamp(min[i], max[i])
            } else {
                min[i]
            };
        }
        self
    }
    /// Monitor-local logical extent with a global desktop origin.
    pub fn on_monitor(mut self, origin: [i32; 2], available: [f64; 2]) -> Self {
        for (i, extent) in available.iter().enumerate() {
            self.size[i] = self.size[i].min(extent.max(1.));
        }
        if let Some(mut p) = self.position {
            for i in 0..2 {
                let end = origin[i].saturating_add((available[i] - self.size[i]).max(0.) as i32);
                p[i] = p[i].clamp(origin[i], end);
            }
            self.position = Some(p);
        }
        self
    }
    pub fn stepped(self, grow: bool) -> Self {
        Self {
            size: self.size.map(|n| n * if grow { 1.1 } else { 1. / 1.1 }),
            ..self
        }
        .limited(true)
    }
}
impl Preferences {
    pub fn path() -> io::Result<PathBuf> {
        Ok(crate::dismissal_store::user_state_directory()?.join("window-preferences.json"))
    }
    pub fn read(path: &Path) -> io::Result<Self> {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        let mut bytes = Vec::new();
        file.take(4097).read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            return Err(io::Error::other("Window preferences exceed size limit"));
        }
        let v: Value = serde_json::from_slice(&bytes)?;
        if v["version"] != 1 {
            return Err(io::Error::other("Unsupported window preferences version"));
        }
        fn bounds(v: &Value) -> Option<Bounds> {
            let size = [v["size"][0].as_f64()?, v["size"][1].as_f64()?];
            let position = if v["position"].is_null() {
                None
            } else {
                Some([
                    i32::try_from(v["position"][0].as_i64()?).ok()?,
                    i32::try_from(v["position"][1].as_i64()?).ok()?,
                ])
            };
            Some(Bounds { size, position })
        }
        Ok(Self {
            normal: bounds(&v["normal"])
                .ok_or_else(|| io::Error::other("Invalid normal geometry"))?
                .limited(false),
            compact: bounds(&v["compact"])
                .ok_or_else(|| io::Error::other("Invalid compact geometry"))?
                .limited(true),
        })
    }
    pub fn write(self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Missing preference directory"))?;
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        let value = json!({"version":1,"normal":{"size":self.normal.size,"position":self.normal.position},"compact":{"size":self.compact.size,"position":self.compact.position}});
        file.write_all(&serde_json::to_vec_pretty(&value)?)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_geometries_survive_restart_and_atomic_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let mut p = Preferences::default();
        p.compact.position = Some([-900, 44]);
        p.normal.size = [1234., 789.];
        p.write(&path).unwrap();
        assert_eq!(Preferences::read(&path).unwrap(), p);
        p.compact.size = [400., 260.];
        p.write(&path).unwrap();
        assert_eq!(Preferences::read(&path).unwrap(), p);
    }
    #[test]
    fn malformed_and_future_preferences_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        for text in ["{", "{\"version\":9}", "{\"version\":1}"] {
            fs::write(&path, text).unwrap();
            assert!(Preferences::read(&path).is_err());
        }
    }
    #[test]
    fn steps_are_bounded_and_removed_monitor_is_recovered() {
        let mut b = Preferences::default().compact;
        for _ in 0..100 {
            b = b.stepped(false);
        }
        assert_eq!(b.size, [360., 240.]);
        for _ in 0..100 {
            b = b.stepped(true);
        }
        assert_eq!(b.size, [960., 720.]);
        b.position = Some([-99999, 99999]);
        b = b.on_monitor([0, 30], [800., 570.]);
        assert_eq!(b.size, [800., 570.]);
        assert_eq!(b.position, Some([0, 30]));
    }
}
