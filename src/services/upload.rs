use crate::models::{Result, AppError, Node};
use crate::db::Database;
use zip::ZipArchive;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

const DATA_DIR: &str = "data";

#[derive(Clone)]
pub struct UploadService {
    db: Database,
}

impl UploadService {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn process_shapefile_upload(
        &self,
        file_path: &str,
        original_name: &str,
        srid_override: Option<String>,
    ) -> Result<Node> {
        info!("Processing shapefile upload: {}", original_name);

        let file = File::open(file_path)?;
        let size = file.metadata()?.len();

        let mut archive = ZipArchive::new(BufReader::new(file))
            .map_err(|e| AppError::FileUpload(format!("Not a valid ZIP file: {}", e)))?;

        let temp_dir = format!("{}/temp_{}", DATA_DIR, chrono::Utc::now().timestamp());

        fs::create_dir_all(&temp_dir)
            .map_err(|e| AppError::Io(format!("Failed to create temp directory: {}", e)))?;

        archive.extract(&temp_dir)
            .map_err(|e| AppError::FileUpload(format!("Failed to extract ZIP: {}", e)))?;

        let shapefile_parts = Self::find_shapefile_parts(Path::new(&temp_dir))?;

        let srid = if let Some(srid) = srid_override {
            let srid = srid.trim().to_string();
            let srid_normalized = srid.to_uppercase();
            let srid_value = if let Some(stripped) = srid_normalized.strip_prefix("EPSG:") {
                stripped.to_string()
            } else {
                srid.clone()
            };
            if srid_value.is_empty() || !srid_value.chars().all(|c| c.is_ascii_digit()) {
                return Err(AppError::FileUpload(
                    "Invalid SRID override; expected numeric EPSG code".to_string(),
                ));
            }
            info!("Using SRID override: {}", srid_value);
            srid_value
        } else {
            let srid = Self::extract_srid_from_prj(&shapefile_parts.prj)?;
            info!("Extracted SRID from .prj file: {}", srid);
            srid
        };

        let base_name = shapefile_parts
            .base_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();

        let table_name = format!("{}_{}", base_name, Self::generate_guid());

        // Use absolute path for shapefile
        let shp_file_abs = std::fs::canonicalize(&shapefile_parts.shp)
            .map_err(|e| AppError::Io(format!("Failed to get absolute path: {}", e)))?;

        self.db.import_shapefile(&table_name, shp_file_abs.to_str().ok_or_else(|| {
            AppError::Io("Failed to convert path to string".to_string())
        })?)?;

        let storage_dir = format!("{}/{}_{}", DATA_DIR, base_name, Self::generate_guid());
        fs::create_dir_all(&storage_dir)
            .map_err(|e| AppError::Io(format!("Failed to create storage directory: {}", e)))?;

        let stored_paths = Self::move_shapefile_parts(&shapefile_parts, &storage_dir)?;

        let node = Node::new_resource(
            base_name.to_string(),
            Some(format!("Uploaded from {}", original_name)),
            stored_paths,
            size,
            srid,
            table_name,
        );

        fs::remove_dir_all(&temp_dir)?;

        info!("Successfully processed shapefile and created resource node: {}", node.id);
        Ok(node)
    }

    fn find_shapefile_parts(dir: &Path) -> Result<ShapefileParts> {
        let files = Self::collect_files(dir)?;

        let shp_files: Vec<PathBuf> = files
            .iter()
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("shp"))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        if shp_files.is_empty() {
            return Err(AppError::FileUpload(
                "No .shp file found in ZIP archive".to_string(),
            ));
        }

        if shp_files.len() > 1 {
            return Err(AppError::FileUpload(
                "Multiple .shp files found; please upload a single shapefile".to_string(),
            ));
        }

        let shp = shp_files[0].clone();
        let base_name = shp
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| AppError::FileUpload("Invalid shapefile name".to_string()))?
            .to_string();

        let base_lower = base_name.to_lowercase();
        let find_part = |ext: &str| -> Option<PathBuf> {
            files.iter().find_map(|path| {
                let stem = path.file_stem()?.to_str()?;
                let extension = path.extension()?.to_str()?;
                if stem.eq_ignore_ascii_case(&base_lower) && extension.eq_ignore_ascii_case(ext) {
                    return Some(path.clone());
                }
                if stem.eq_ignore_ascii_case(&base_name) && extension.eq_ignore_ascii_case(ext) {
                    return Some(path.clone());
                }
                None
            })
        };

        let shx = find_part("shx");
        let dbf = find_part("dbf");
        let prj = find_part("prj");
        let cpg = find_part("cpg");

        if shx.is_none() {
            return Err(AppError::FileUpload(
                "Missing required .shx file in shapefile archive".to_string(),
            ));
        }

        if dbf.is_none() {
            return Err(AppError::FileUpload(
                "Missing required .dbf file in shapefile archive".to_string(),
            ));
        }

        Ok(ShapefileParts {
            base_name,
            shp,
            shx: shx.unwrap(),
            dbf: dbf.unwrap(),
            prj,
            cpg,
        })
    }

    fn collect_files(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let entries = fs::read_dir(dir)
            .map_err(|e| AppError::Io(format!("Failed to read directory: {}", e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| AppError::Io(format!("Failed to read entry: {}", e)))?;
            let path = entry.path();
            if path.is_dir() {
                files.extend(Self::collect_files(&path)?);
            } else {
                files.push(path);
            }
        }

        Ok(files)
    }

    fn extract_srid_from_prj(prj_file: &Option<PathBuf>) -> Result<String> {
        match prj_file {
            Some(path) => {
                let content = fs::read_to_string(path)
                    .map_err(|e| AppError::Parse(format!("Failed to read .prj file: {}", e)))?;

                let srid = Self::parse_srid_from_wkt(&content)?;
                Ok(srid)
            }
            None => Err(AppError::FileUpload(
                "No .prj file found and no SRID override provided".to_string(),
            )),
        }
    }

    fn parse_srid_from_wkt(wkt: &str) -> Result<String> {
        let wkt_lower = wkt.to_lowercase();

        let re = regex::Regex::new(r#"(?i)AUTHORITY\["(EPSG|ESRI)",\s*(\d+)\]"#).unwrap();
        if let Some(caps) = re.captures(wkt) {
            return Ok(caps[2].to_string());
        }

        let common_srids = [
            ("PROJCS[\"WGS 84 / Pseudo-Mercator\"", "3857"),
            ("PROJCS[\"WGS 84 / UTM zone 50N\"", "32650"),
            ("PROJCS[\"NAD83 / UTM zone 10N\"", "26910"),
        ];

        for (pattern, srid) in &common_srids {
            if wkt_lower.contains(&pattern.to_lowercase()) {
                return Ok(srid.to_string());
            }
        }

        if wkt_lower.contains("wgs 84") || wkt_lower.contains("epsg:4326") {
            return Ok("4326".to_string());
        }

        Err(AppError::FileUpload(
            "Could not auto-detect SRID from .prj; please provide SRID override".to_string(),
        ))
    }

    fn generate_guid() -> String {
        use uuid::Uuid;
        format!("{:X}", Uuid::new_v4()).replace('-', "_")
    }

    fn move_shapefile_parts(parts: &ShapefileParts, storage_dir: &str) -> Result<Vec<String>> {
        let mut stored_paths = Vec::new();
        let targets = parts.all_paths();

        for source in targets {
            let file_name = source
                .file_name()
                .ok_or_else(|| AppError::Io("Invalid shapefile path".to_string()))?;
            let dest = Path::new(storage_dir).join(file_name);

            if let Err(err) = fs::rename(&source, &dest) {
                warn!("Rename failed ({}), falling back to copy", err);
                fs::copy(&source, &dest).map_err(|e| {
                    AppError::Io(format!("Failed to copy shapefile part: {}", e))
                })?;
                fs::remove_file(&source).map_err(|e| {
                    AppError::Io(format!("Failed to remove original shapefile part: {}", e))
                })?;
            }

            stored_paths.push(dest.to_string_lossy().to_string());
        }

        Ok(stored_paths)
    }
}

struct ShapefileParts {
    base_name: String,
    shp: PathBuf,
    shx: PathBuf,
    dbf: PathBuf,
    prj: Option<PathBuf>,
    cpg: Option<PathBuf>,
}

impl ShapefileParts {
    fn all_paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![self.shp.clone(), self.shx.clone(), self.dbf.clone()];
        if let Some(prj) = &self.prj {
            paths.push(prj.clone());
        }
        if let Some(cpg) = &self.cpg {
            paths.push(cpg.clone());
        }
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_srid_from_wkt() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]]"#;
        let srid = UploadService::parse_srid_from_wkt(wkt).unwrap();
        assert_eq!(srid, "4326");
    }

    #[test]
    fn test_parse_srid_web_mercator() {
        let wkt = r#"PROJCS["WGS 84 / Pseudo-Mercator",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]],PROJECTION["Mercator_1SP"],PARAMETER["central_meridian",0],PARAMETER["scale_factor",1],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["metre",1,AUTHORITY["EPSG","9001"]],AXIS["Easting",EAST],AXIS["Northing",NORTH],AUTHORITY["EPSG","3857"]]"#;
        let srid = UploadService::parse_srid_from_wkt(wkt).unwrap();
        assert_eq!(srid, "3857");
    }

    #[test]
    fn test_find_shapefile_no_shp() {
        let result = UploadService::find_shapefile_parts(Path::new("/nonexistent"));
        assert!(result.is_err());
    }
}
