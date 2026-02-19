use lazy_static::lazy_static;
use regex::Regex;

pub const CRS_TYPE_STANDARD: &str = "standard";
pub const CRS_TYPE_CUSTOM: &str = "custom";

lazy_static! {
    static ref EPSG_PATTERN: Regex = Regex::new(r"(?i)^EPSG:(\d+)$").unwrap();
    static ref WGS84_ALIASES: Vec<&'static str> = vec![
        "WGS84",
        "WGS_1984",
        "CRS84",
        "urn:ogc:def:crs:OGC:1.3:CRS84"
    ];
    static ref AUTHORITY_EPSG_PATTERN: Regex =
        Regex::new(r#"AUTHORITY\s*\[\s*"EPSG"\s*,\s*"(\d+)"\s*\]"#).unwrap();
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedCrs {
    pub crs: Option<String>,
    pub crs_type: String,
}

pub fn normalize_crs(raw: Option<&str>) -> NormalizedCrs {
    match raw {
        None => NormalizedCrs {
            crs: None,
            crs_type: CRS_TYPE_CUSTOM.to_string(),
        },
        Some(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return NormalizedCrs {
                    crs: None,
                    crs_type: CRS_TYPE_CUSTOM.to_string(),
                };
            }

            if let Some(caps) = EPSG_PATTERN.captures(trimmed) {
                let code: &str = caps.get(1).unwrap().as_str();
                return NormalizedCrs {
                    crs: Some(format!("EPSG:{}", code)),
                    crs_type: CRS_TYPE_STANDARD.to_string(),
                };
            }

            if WGS84_ALIASES
                .iter()
                .any(|alias| trimmed.eq_ignore_ascii_case(alias))
            {
                return NormalizedCrs {
                    crs: Some("EPSG:4326".to_string()),
                    crs_type: CRS_TYPE_STANDARD.to_string(),
                };
            }

            if trimmed.starts_with("PROJCS") || trimmed.starts_with("GEOGCS") {
                return parse_wkt(trimmed);
            }

            NormalizedCrs {
                crs: Some(trimmed.to_string()),
                crs_type: CRS_TYPE_CUSTOM.to_string(),
            }
        }
    }
}

fn parse_wkt(wkt: &str) -> NormalizedCrs {
    if let Some(caps) = AUTHORITY_EPSG_PATTERN.captures(wkt) {
        let code: &str = caps.get(1).unwrap().as_str();
        NormalizedCrs {
            crs: Some(format!("EPSG:{}", code)),
            crs_type: CRS_TYPE_STANDARD.to_string(),
        }
    } else {
        NormalizedCrs {
            crs: Some(wkt.to_string()),
            crs_type: CRS_TYPE_CUSTOM.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DataBounds {
    pub minx: f64,
    pub miny: f64,
    pub maxx: f64,
    pub maxy: f64,
}

impl DataBounds {
    pub fn to_json(self) -> String {
        format!(
            r#"{{"minx":{},"miny":{},"maxx":{},"maxy":{}}}"#,
            self.minx, self.miny, self.maxx, self.maxy
        )
    }

    pub fn from_json(json: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        Some(DataBounds {
            minx: v.get("minx")?.as_f64()?,
            miny: v.get("miny")?.as_f64()?,
            maxx: v.get("maxx")?.as_f64()?,
            maxy: v.get("maxy")?.as_f64()?,
        })
    }

    pub fn to_array(self) -> [f64; 4] {
        [self.minx, self.miny, self.maxx, self.maxy]
    }

    pub fn is_valid(&self) -> bool {
        self.maxx > self.minx && self.maxy > self.miny
    }

    pub fn is_valid_wgs84(&self) -> bool {
        self.minx >= -180.0 && self.maxx <= 180.0 && self.miny >= -90.0 && self.maxy <= 90.0
    }
}

pub fn calculate_custom_tile_bbox(
    bounds: &DataBounds,
    z: i32,
    x: i32,
    y: i32,
) -> (f64, f64, f64, f64) {
    let tiles_per_side = 2_f64.powi(z);
    let tile_width = (bounds.maxx - bounds.minx) / tiles_per_side;
    let tile_height = (bounds.maxy - bounds.miny) / tiles_per_side;

    let minx = bounds.minx + x as f64 * tile_width;
    let maxx = bounds.minx + (x + 1) as f64 * tile_width;
    let maxy = bounds.maxy - y as f64 * tile_height;
    let miny = bounds.maxy - (y + 1) as f64 * tile_height;

    (minx, miny, maxx, maxy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_epsg_code() {
        let result = normalize_crs(Some("EPSG:4326"));
        assert_eq!(result.crs, Some("EPSG:4326".to_string()));
        assert_eq!(result.crs_type, CRS_TYPE_STANDARD);

        let result = normalize_crs(Some("epsg:3857"));
        assert_eq!(result.crs, Some("EPSG:3857".to_string()));
        assert_eq!(result.crs_type, CRS_TYPE_STANDARD);
    }

    #[test]
    fn test_normalize_wgs84_alias() {
        let result = normalize_crs(Some("WGS84"));
        assert_eq!(result.crs, Some("EPSG:4326".to_string()));
        assert_eq!(result.crs_type, CRS_TYPE_STANDARD);

        let result = normalize_crs(Some("CRS84"));
        assert_eq!(result.crs, Some("EPSG:4326".to_string()));
        assert_eq!(result.crs_type, CRS_TYPE_STANDARD);

        let result = normalize_crs(Some("urn:ogc:def:crs:OGC:1.3:CRS84"));
        assert_eq!(result.crs, Some("EPSG:4326".to_string()));
        assert_eq!(result.crs_type, CRS_TYPE_STANDARD);
    }

    #[test]
    fn test_normalize_none() {
        let result = normalize_crs(None);
        assert_eq!(result.crs, None);
        assert_eq!(result.crs_type, CRS_TYPE_CUSTOM);
    }

    #[test]
    fn test_normalize_custom_name() {
        let result = normalize_crs(Some("LOCAL_GRID"));
        assert_eq!(result.crs, Some("LOCAL_GRID".to_string()));
        assert_eq!(result.crs_type, CRS_TYPE_CUSTOM);
    }

    #[test]
    fn test_normalize_wkt_with_epsg() {
        let wkt = r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["Degree",0.017453292519943295],AUTHORITY["EPSG","4326"]]"#;
        let result = normalize_crs(Some(wkt));
        assert_eq!(result.crs, Some("EPSG:4326".to_string()));
        assert_eq!(result.crs_type, CRS_TYPE_STANDARD);
    }

    #[test]
    fn test_normalize_wkt_without_epsg() {
        let wkt = r#"PROJCS["Local_Grid",GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["Degree",0.017453292519943295]],PROJECTION["Transverse_Mercator"],UNIT["Meter",1]]"#;
        let result = normalize_crs(Some(wkt));
        assert_eq!(result.crs, Some(wkt.to_string()));
        assert_eq!(result.crs_type, CRS_TYPE_CUSTOM);
    }

    #[test]
    fn test_data_bounds_json() {
        let bounds = DataBounds {
            minx: 1000.0,
            miny: 2000.0,
            maxx: 1500.0,
            maxy: 2500.0,
        };
        let json = bounds.to_json();
        assert_eq!(json, r#"{"minx":1000,"miny":2000,"maxx":1500,"maxy":2500}"#);

        let parsed = DataBounds::from_json(&json).unwrap();
        assert_eq!(parsed.minx, 1000.0);
        assert_eq!(parsed.maxy, 2500.0);
    }

    #[test]
    fn test_calculate_custom_tile_bbox() {
        let bounds = DataBounds {
            minx: 1000.0,
            miny: 2000.0,
            maxx: 1500.0,
            maxy: 2500.0,
        };

        let (minx, miny, maxx, maxy) = calculate_custom_tile_bbox(&bounds, 0, 0, 0);
        assert!((minx - 1000.0).abs() < 1e-10);
        assert!((miny - 2000.0).abs() < 1e-10);
        assert!((maxx - 1500.0).abs() < 1e-10);
        assert!((maxy - 2500.0).abs() < 1e-10);

        let (minx, miny, maxx, maxy) = calculate_custom_tile_bbox(&bounds, 1, 0, 0);
        assert!((minx - 1000.0).abs() < 1e-10);
        assert!((maxx - 1250.0).abs() < 1e-10);
        assert!((miny - 2250.0).abs() < 1e-10);
        assert!((maxy - 2500.0).abs() < 1e-10);

        let (minx, miny, maxx, maxy) = calculate_custom_tile_bbox(&bounds, 1, 1, 1);
        assert!((minx - 1250.0).abs() < 1e-10);
        assert!((maxx - 1500.0).abs() < 1e-10);
        assert!((miny - 2000.0).abs() < 1e-10);
        assert!((maxy - 2250.0).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_custom_tile_bbox_negative_coords() {
        let bounds = DataBounds {
            minx: -500.0,
            miny: -300.0,
            maxx: -400.0,
            maxy: -200.0,
        };

        let (minx, miny, maxx, maxy) = calculate_custom_tile_bbox(&bounds, 0, 0, 0);
        assert!((minx - (-500.0)).abs() < 1e-10);
        assert!((miny - (-300.0)).abs() < 1e-10);
        assert!((maxx - (-400.0)).abs() < 1e-10);
        assert!((maxy - (-200.0)).abs() < 1e-10);
    }

    #[test]
    fn test_data_bounds_is_valid() {
        let valid = DataBounds {
            minx: 0.0,
            miny: 0.0,
            maxx: 100.0,
            maxy: 100.0,
        };
        assert!(valid.is_valid());

        let zero_extent = DataBounds {
            minx: 0.0,
            miny: 0.0,
            maxx: 0.0,
            maxy: 0.0,
        };
        assert!(!zero_extent.is_valid());

        let negative_extent = DataBounds {
            minx: 100.0,
            miny: 100.0,
            maxx: 0.0,
            maxy: 0.0,
        };
        assert!(!negative_extent.is_valid());
    }

    #[test]
    fn test_is_valid_wgs84() {
        let wgs84_bounds = DataBounds {
            minx: -74.1,
            miny: 40.5,
            maxx: -73.9,
            maxy: 40.9,
        };
        assert!(wgs84_bounds.is_valid_wgs84());

        let global_bounds = DataBounds {
            minx: -180.0,
            miny: -90.0,
            maxx: 180.0,
            maxy: 90.0,
        };
        assert!(global_bounds.is_valid_wgs84());

        let out_of_range_x = DataBounds {
            minx: 1000.0,
            miny: 0.0,
            maxx: 1500.0,
            maxy: 100.0,
        };
        assert!(!out_of_range_x.is_valid_wgs84());

        let out_of_range_y = DataBounds {
            minx: 0.0,
            miny: -300.0,
            maxx: 100.0,
            maxy: -200.0,
        };
        assert!(!out_of_range_y.is_valid_wgs84());

        let negative_coords = DataBounds {
            minx: -500.0,
            miny: -300.0,
            maxx: -400.0,
            maxy: -200.0,
        };
        assert!(!negative_coords.is_valid_wgs84());
    }
}
