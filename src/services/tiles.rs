use crate::models::{Result, Config, NodeType};
use crate::db::Database;
use bytes::Bytes;
use tracing::{info, debug};

#[derive(Clone)]
pub struct TileService {
    db: Database,
}

impl TileService {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn generate_tile(&self, config: &Config, z: u32, x: u32, y: u32) -> Result<Option<Bytes>> {
        debug!("Generating tile z={}, x={}, y={}", z, x, y);

        let mut tile_bytes: Vec<u8> = Vec::new();
        let mut has_data = false;

        for node in &config.nodes {
            if node.node_type == NodeType::Xyz {
                if let Some(data) = node.get_xyz_data() {
                    if z < data.min_zoom || z > data.max_zoom {
                        continue;
                    }

                    for layer_ref in &data.layers {
                        if let Some(layer_node) = config.find_node(&layer_ref.source_layer_id) {
                            if let Some(layer_data) = layer_node.get_layer_data() {
                                if z < layer_data.minzoom || z > layer_data.maxzoom {
                                    continue;
                                }

                                if let Some(resource_node) = config.find_node(&layer_data.source_resource_id) {
                                    if let Some(resource_data) = resource_node.get_resource_data() {
                                        let fields = layer_data.fields.as_ref().map(|f| f.as_slice()).unwrap_or(&[]);

                                        match self.db.generate_tile(z, x, y, &resource_data.duckdb_table_name, fields)? {
                                            Some(mut data) => {
                                                tile_bytes.append(&mut data);
                                                has_data = true;
                                            }
                                            None => {
                                                debug!("No data for table {} at z={}, x={}, y={}", resource_data.duckdb_table_name, z, x, y);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if has_data {
            info!("Generated tile z={}, x={}, y={}, size: {} bytes", z, x, y, tile_bytes.len());
            Ok(Some(Bytes::from(tile_bytes)))
        } else {
            debug!("No data for tile z={}, x={}, y={}", z, x, y);
            Ok(None)
        }
    }

    pub async fn get_tile_metadata(&self, config: &Config) -> Result<TileMetadata> {
        let mut min_zoom = u32::MAX;
        let mut max_zoom = 0u32;
        let mut bounds: Option<(f64, f64, f64, f64)> = None;

        for node in &config.nodes {
            if node.node_type == NodeType::Xyz {
                if let Some(data) = node.get_xyz_data() {
                    min_zoom = min_zoom.min(data.min_zoom);
                    max_zoom = max_zoom.max(data.max_zoom);

                    if bounds.is_none() {
                        bounds = Some((
                            data.bounds[0],
                            data.bounds[1],
                            data.bounds[2],
                            data.bounds[3],
                        ));
                    } else {
                        let b = bounds.unwrap();
                        bounds = Some((
                            b.0.min(data.bounds[0]),
                            b.1.min(data.bounds[1]),
                            b.2.max(data.bounds[2]),
                            b.3.max(data.bounds[3]),
                        ));
                    }
                }
            }
        }

        let metadata = TileMetadata {
            min_zoom: if min_zoom == u32::MAX { 0 } else { min_zoom },
            max_zoom,
            bounds,
        };

        Ok(metadata)
    }
}

#[derive(Debug, Clone)]
pub struct TileMetadata {
    pub min_zoom: u32,
    pub max_zoom: u32,
    pub bounds: Option<(f64, f64, f64, f64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Node;

    #[tokio::test]
    async fn test_tile_metadata() {
        let db = Database::new().unwrap();
        let service = TileService::new(db);

        let mut config = Config::new();
        let xyz = Node::new_xyz(
            "test".to_string(),
            None,
            vec![121.4737, 31.2304, 12.0],
            0,
            22,
            12,
            vec![-180.0, -85.0511, 180.0, 85.0511],
            vec![],
        );
        config.add_node(xyz);

        let metadata = service.get_tile_metadata(&config).await;
        assert!(metadata.is_ok());
        let meta = metadata.unwrap();
        assert_eq!(meta.min_zoom, 0);
        assert_eq!(meta.max_zoom, 22);
        assert!(meta.bounds.is_some());
    }
}
