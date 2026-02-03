use crate::db::Database;
use crate::models::{AppError, Config, Edge, NodeType, Result};
use std::fs;
use std::path::Path;
use tracing::{info, warn};

const CONFIG_FILE: &str = "config.json";
const SUPPORTED_CONFIG_VERSION: &str = "0.1.0";

#[derive(Debug, Clone)]
pub struct VerifyError {
    pub node_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub valid: bool,
    pub errors: Vec<VerifyError>,
}

#[derive(Clone)]
pub struct ConfigService {
    db: Database,
}

impl ConfigService {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn load_config(&self) -> Result<Config> {
        let path = Path::new(CONFIG_FILE);

        if !path.exists() {
            warn!(
                "Config file '{}' not found, returning empty config",
                CONFIG_FILE
            );
            return Ok(Config::new());
        }

        let content = fs::read_to_string(path)
            .map_err(|e| AppError::Io(format!("Failed to read config file: {}", e)))?;

        let config: Config = serde_json::from_str(&content)
            .map_err(|e| AppError::Parse(format!("Failed to parse config: {}", e)))?;

        info!(
            "Loaded config with {} nodes and {} edges",
            config.nodes.len(),
            config.edges.len()
        );
        Ok(config)
    }

    pub fn save_config(&self, config: &Config) -> Result<()> {
        let content = serde_json::to_string_pretty(config)
            .map_err(|e| AppError::Parse(format!("Failed to serialize config: {}", e)))?;

        let temp_path = format!("{}.tmp", CONFIG_FILE);
        fs::write(&temp_path, &content)
            .map_err(|e| AppError::Io(format!("Failed to write temp config: {}", e)))?;

        fs::rename(&temp_path, CONFIG_FILE)
            .map_err(|e| AppError::Io(format!("Failed to rename config file: {}", e)))?;

        info!(
            "Saved config with {} nodes and {} edges",
            config.nodes.len(),
            config.edges.len()
        );
        Ok(())
    }

    pub fn verify(&self, config: &Config) -> VerifyResult {
        let mut errors = Vec::new();

        errors.extend(self.verify_version(config));
        errors.extend(self.verify_node_ids(config));
        errors.extend(self.verify_node_type_matches_data(config));
        errors.extend(self.verify_edge_references(config));
        errors.extend(self.verify_edge_types(config));
        errors.extend(self.verify_node_data(config));
        errors.extend(self.verify_edge_data_consistency(config));
        errors.extend(self.verify_circular_dependencies(config));

        VerifyResult {
            valid: errors.is_empty(),
            errors,
        }
    }

    fn verify_node_ids(&self, config: &Config) -> Vec<VerifyError> {
        let mut errors = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for node in &config.nodes {
            if node.id.is_empty() {
                errors.push(VerifyError {
                    node_id: Some(node.id.clone()),
                    message: "Node ID cannot be empty".to_string(),
                });
            } else if seen_ids.contains(&node.id) {
                errors.push(VerifyError {
                    node_id: Some(node.id.clone()),
                    message: format!("Duplicate node ID: {}", node.id),
                });
            } else {
                seen_ids.insert(node.id.clone());
            }
        }

        errors
    }

    fn verify_version(&self, config: &Config) -> Vec<VerifyError> {
        if config.version != SUPPORTED_CONFIG_VERSION {
            vec![VerifyError {
                node_id: None,
                message: format!(
                    "Unsupported config version: {} (expected {})",
                    config.version, SUPPORTED_CONFIG_VERSION
                ),
            }]
        } else {
            Vec::new()
        }
    }

    fn verify_node_type_matches_data(&self, config: &Config) -> Vec<VerifyError> {
        let mut errors = Vec::new();

        for node in &config.nodes {
            if !node.data_matches_type() {
                errors.push(VerifyError {
                    node_id: Some(node.id.clone()),
                    message: "Node type does not match node data".to_string(),
                });
            }
        }

        errors
    }

    fn verify_edge_references(&self, config: &Config) -> Vec<VerifyError> {
        let mut errors = Vec::new();
        let node_ids: std::collections::HashSet<_> = config.nodes.iter().map(|n| &n.id).collect();

        for edge in &config.edges {
            if !node_ids.contains(&edge.source) {
                errors.push(VerifyError {
                    node_id: None,
                    message: format!("Edge references non-existent source node: {}", edge.source),
                });
            }

            if !node_ids.contains(&edge.target) {
                errors.push(VerifyError {
                    node_id: None,
                    message: format!("Edge references non-existent target node: {}", edge.target),
                });
            }
        }

        errors
    }

    fn verify_edge_types(&self, config: &Config) -> Vec<VerifyError> {
        let mut errors = Vec::new();

        let node_map: std::collections::HashMap<_, _> =
            config.nodes.iter().map(|n| (&n.id, &n.node_type)).collect();

        for edge in &config.edges {
            let source_type = node_map.get(&edge.source);
            let target_type = node_map.get(&edge.target);

            match (source_type, target_type) {
                (Some(NodeType::Resource), Some(NodeType::Layer)) => {}
                (Some(NodeType::Layer), Some(NodeType::Xyz)) => {}
                (Some(NodeType::Resource), Some(NodeType::Xyz)) => {
                    errors.push(VerifyError {
                        node_id: Some(edge.target.clone()),
                        message: "XYZ node cannot connect directly to resource".to_string(),
                    });
                }
                (Some(NodeType::Layer), Some(NodeType::Layer)) => {
                    errors.push(VerifyError {
                        node_id: Some(edge.target.clone()),
                        message: "Layer node cannot connect to another layer".to_string(),
                    });
                }
                (Some(NodeType::Xyz), Some(NodeType::Xyz)) => {
                    errors.push(VerifyError {
                        node_id: Some(edge.target.clone()),
                        message: "XYZ node cannot connect to another XYZ".to_string(),
                    });
                }
                (Some(NodeType::Resource), Some(NodeType::Resource)) => {
                    errors.push(VerifyError {
                        node_id: Some(edge.target.clone()),
                        message: "Resource node cannot connect to another resource".to_string(),
                    });
                }
                (Some(NodeType::Xyz), Some(NodeType::Layer)) => {
                    errors.push(VerifyError {
                        node_id: Some(edge.target.clone()),
                        message: "Layer cannot connect to XYZ (wrong direction)".to_string(),
                    });
                }
                (Some(NodeType::Layer), Some(NodeType::Resource)) => {
                    errors.push(VerifyError {
                        node_id: Some(edge.target.clone()),
                        message: "Resource cannot connect to Layer (wrong direction)".to_string(),
                    });
                }
                _ => {}
            }
        }

        errors
    }

    fn verify_node_data(&self, config: &Config) -> Vec<VerifyError> {
        let mut errors = Vec::new();

        for node in &config.nodes {
            match &node.node_type {
                NodeType::Resource => {
                    if let Some(data) = node.get_resource_data() {
                        if data.file_path.is_empty() {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "Resource node must have at least one file path"
                                    .to_string(),
                            });
                        }

                        if data.srid.trim().is_empty() {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "Resource node must have a valid SRID".to_string(),
                            });
                        }

                        if data.resource_type != "shapefile" {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "Resource type must be 'shapefile'".to_string(),
                            });
                        }

                        if data.duckdb_table_name.trim().is_empty() {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "Resource node must include a DuckDB table name".to_string(),
                            });
                        }

                        if let Err(e) = self.db.table_exists(&data.duckdb_table_name) {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: format!(
                                    "Failed to check table existence: {}",
                                    e.message()
                                ),
                            });
                        }
                    }
                }
                NodeType::Layer => {
                    if let Some(data) = node.get_layer_data() {
                        if data.source_resource_id.trim().is_empty() {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "Layer node must reference a resource".to_string(),
                            });
                        }

                        if data.minzoom >= data.maxzoom {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: format!(
                                    "minzoom ({}) must be less than maxzoom ({})",
                                    data.minzoom, data.maxzoom
                                ),
                            });
                        }

                        if data.maxzoom > 22 {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "maxzoom cannot exceed 22".to_string(),
                            });
                        }

                        if config.find_node(&data.source_resource_id).is_none() {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: format!(
                                    "Source resource node not found: {}",
                                    data.source_resource_id
                                ),
                            });
                        } else if let Some(fields) = &data.fields {
                            if let Some(resource_node) = config.find_node(&data.source_resource_id)
                            {
                                if let Some(resource_data) = resource_node.get_resource_data() {
                                    match self.db.table_exists(&resource_data.duckdb_table_name) {
                                        Ok(true) => match self
                                            .db
                                            .get_table_info(&resource_data.duckdb_table_name)
                                        {
                                            Ok(columns) => {
                                                let column_set: std::collections::HashSet<_> =
                                                    columns.iter().collect();
                                                for field in fields {
                                                    if !column_set.contains(field) {
                                                        errors.push(VerifyError {
                                                            node_id: Some(node.id.clone()),
                                                            message: format!(
                                                                "Field '{}' not found in table {}",
                                                                field, resource_data.duckdb_table_name
                                                            ),
                                                        });
                                                    }
                                                }
                                            }
                                            Err(e) => errors.push(VerifyError {
                                                node_id: Some(node.id.clone()),
                                                message: format!(
                                                    "Failed to read table fields: {}",
                                                    e.message()
                                                ),
                                            }),
                                        },
                                        Ok(false) => {}
                                        Err(e) => errors.push(VerifyError {
                                            node_id: Some(node.id.clone()),
                                            message: format!(
                                                "Failed to check table existence: {}",
                                                e.message()
                                            ),
                                        }),
                                    }
                                }
                            }
                        }
                    }
                }
                NodeType::Xyz => {
                    if let Some(data) = node.get_xyz_data() {
                        if data.bounds.len() != 4 {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message:
                                    "Bounds must have exactly 4 elements [minx, miny, maxx, maxy]"
                                        .to_string(),
                            });
                        }

                        if data.center.len() != 3 {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "Center must have exactly 3 elements [lon, lat, zoom]"
                                    .to_string(),
                            });
                        }

                        if data.min_zoom >= data.max_zoom {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: format!(
                                    "min_zoom ({}) must be less than max_zoom ({})",
                                    data.min_zoom, data.max_zoom
                                ),
                            });
                        }

                        if data.fillzoom < data.min_zoom || data.fillzoom > data.max_zoom {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: format!(
                                    "fillzoom ({}) must be within min_zoom ({}) and max_zoom ({})",
                                    data.fillzoom, data.min_zoom, data.max_zoom
                                ),
                            });
                        }

                        if data.bounds.len() == 4 {
                            let (minx, miny, maxx, maxy) =
                                (data.bounds[0], data.bounds[1], data.bounds[2], data.bounds[3]);
                            if minx >= maxx || miny >= maxy {
                                errors.push(VerifyError {
                                    node_id: Some(node.id.clone()),
                                    message: "Bounds min values must be less than max values"
                                        .to_string(),
                                });
                            }
                        }

                        if data.center.len() == 3 {
                            let zoom = data.center[2];
                            if zoom < data.min_zoom as f64 || zoom > data.max_zoom as f64 {
                                errors.push(VerifyError {
                                    node_id: Some(node.id.clone()),
                                    message: "Center zoom must be within min_zoom and max_zoom"
                                        .to_string(),
                                });
                            }
                        }

                        if data.layers.is_empty() {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "XYZ node must include at least one layer".to_string(),
                            });
                        }

                        for layer_ref in &data.layers {
                            if layer_ref.id.trim().is_empty() {
                                errors.push(VerifyError {
                                    node_id: Some(node.id.clone()),
                                    message: "Layer reference id cannot be empty".to_string(),
                                });
                            }

                            if config.find_node(&layer_ref.source_layer_id).is_none() {
                                errors.push(VerifyError {
                                    node_id: Some(node.id.clone()),
                                    message: format!(
                                        "Layer reference not found: {}",
                                        layer_ref.source_layer_id
                                    ),
                                });
                            }
                        }

                        let mut seen_layers = std::collections::HashSet::new();
                        for layer_ref in &data.layers {
                            if !seen_layers.insert(&layer_ref.source_layer_id) {
                                errors.push(VerifyError {
                                    node_id: Some(node.id.clone()),
                                    message: format!(
                                        "Duplicate layer reference: {}",
                                        layer_ref.source_layer_id
                                    ),
                                });
                            }
                        }
                    }
                }
            }
        }

        errors
    }

    fn verify_edge_data_consistency(&self, config: &Config) -> Vec<VerifyError> {
        let mut errors = Vec::new();

        let node_map: std::collections::HashMap<_, _> =
            config.nodes.iter().map(|n| (&n.id, n)).collect();

        let mut incoming: std::collections::HashMap<&str, Vec<&Edge>> =
            std::collections::HashMap::new();
        for edge in &config.edges {
            incoming
                .entry(edge.target.as_str())
                .or_default()
                .push(edge);
        }

        for node in &config.nodes {
            match node.node_type {
                NodeType::Layer => {
                    if let Some(data) = node.get_layer_data() {
                        let incoming_edges =
                            incoming.get(node.id.as_str()).cloned().unwrap_or_default();
                        let resource_incoming: Vec<&Edge> = incoming_edges
                            .iter()
                            .copied()
                            .filter(|edge| {
                                node_map
                                    .get(&edge.source)
                                    .map(|n| n.node_type == NodeType::Resource)
                                    .unwrap_or(false)
                            })
                            .collect();

                        if resource_incoming.is_empty() {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "Layer node must be connected from a resource".to_string(),
                            });
                        } else if resource_incoming.len() > 1 {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "Layer node can only have one resource input".to_string(),
                            });
                        } else if data.source_resource_id != resource_incoming[0].source {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: "Layer source_resource_id does not match its incoming edge"
                                    .to_string(),
                            });
                        }
                    }
                }
                NodeType::Xyz => {
                    if let Some(data) = node.get_xyz_data() {
                        let incoming_edges =
                            incoming.get(node.id.as_str()).cloned().unwrap_or_default();
                        let layer_incoming: std::collections::HashSet<String> = incoming_edges
                            .iter()
                            .filter(|edge| {
                                node_map
                                    .get(&edge.source)
                                    .map(|n| n.node_type == NodeType::Layer)
                                    .unwrap_or(false)
                            })
                            .map(|edge| edge.source.clone())
                            .collect();

                        let layer_refs: std::collections::HashSet<String> = data
                            .layers
                            .iter()
                            .map(|layer| layer.source_layer_id.clone())
                            .collect();

                        for layer_id in layer_refs.difference(&layer_incoming) {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: format!(
                                    "XYZ node references layer '{}' without a connecting edge",
                                    layer_id
                                ),
                            });
                        }

                        for layer_id in layer_incoming.difference(&layer_refs) {
                            errors.push(VerifyError {
                                node_id: Some(node.id.clone()),
                                message: format!(
                                    "XYZ node has incoming edge from layer '{}' not listed in layers",
                                    layer_id
                                ),
                            });
                        }
                    }
                }
                NodeType::Resource => {}
            }
        }

        errors
    }

    fn verify_circular_dependencies(&self, config: &Config) -> Vec<VerifyError> {
        let mut errors = Vec::new();

        let adjacency_list: std::collections::HashMap<_, Vec<_>> = config
            .edges
            .iter()
            .map(|e| (e.source.clone(), e.target.clone()))
            .fold(std::collections::HashMap::new(), |mut acc, (s, t)| {
                acc.entry(s).or_default().push(t);
                acc
            });

        for node in &config.nodes {
            if let Some(path) = self.find_cycle(
                &adjacency_list,
                &node.id,
                &mut std::collections::HashSet::new(),
            ) {
                errors.push(VerifyError {
                    node_id: Some(path[0].clone()),
                    message: format!("Circular dependency detected: {}", path.join(" -> ")),
                });
            }
        }

        errors
    }

    fn find_cycle(
        &self,
        adjacency_list: &std::collections::HashMap<String, Vec<String>>,
        current: &str,
        visited: &mut std::collections::HashSet<String>,
    ) -> Option<Vec<String>> {
        if visited.contains(current) {
            return Some(vec![current.to_string()]);
        }

        let mut visited = visited.clone();
        visited.insert(current.to_string());

        if let Some(neighbors) = adjacency_list.get(current) {
            for neighbor in neighbors {
                if let Some(mut path) = self.find_cycle(adjacency_list, neighbor, &mut visited) {
                    path.insert(0, current.to_string());
                    return Some(path);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Edge, Node, XyzLayerRef};

    #[test]
    fn test_config_save_load() {
        let db = Database::new().unwrap();
        let service = ConfigService::new(db);

        let mut config = Config::new();
        let node = Node::new_resource(
            "test".to_string(),
            None,
            vec!["test.shp".to_string()],
            1000,
            "4326".to_string(),
            "test_table".to_string(),
        );
        config.add_node(node);

        let result = service.save_config(&config);
        assert!(result.is_ok());

        let loaded = service.load_config();
        assert!(loaded.is_ok());
        let loaded_config = loaded.unwrap();
        assert_eq!(loaded_config.nodes.len(), 1);
        assert_eq!(loaded_config.nodes[0].name, "test");
    }

    #[test]
    fn test_verify_valid_config() {
        let db = Database::new().unwrap();
        let service = ConfigService::new(db);

        let mut config = Config::new();
        let resource = Node::new_resource(
            "shanghai".to_string(),
            None,
            vec!["test.shp".to_string()],
            1000,
            "4326".to_string(),
            "test_table".to_string(),
        );
        config.add_node(resource.clone());

        let layer = Node::new_layer(
            "buildings".to_string(),
            None,
            resource.id.clone(),
            Some(vec!["name".to_string()]),
            10,
            18,
        );
        config.add_node(layer.clone());

        let xyz = Node::new_xyz(
            "shanghai_tiles".to_string(),
            None,
            vec![121.4737, 31.2304, 12.0],
            0,
            22,
            12,
            vec![-180.0, -85.0511, 180.0, 85.0511],
            vec![XyzLayerRef {
                id: "layer_buildings".to_string(),
                source_layer_id: layer.id.clone(),
            }],
        );
        config.add_node(xyz.clone());

        config.add_edge(Edge::new(resource.id.clone(), layer.id.clone()));
        config.add_edge(Edge::new(layer.id, xyz.id));

        let result = service.verify(&config);
        assert!(result.valid);
    }

    #[test]
    fn test_verify_invalid_zoom_levels() {
        let db = Database::new().unwrap();
        let service = ConfigService::new(db);

        let mut config = Config::new();
        let layer = Node::new_layer(
            "test".to_string(),
            None,
            "RES_1".to_string(),
            None,
            18, // minzoom >= maxzoom
            10,
        );
        config.add_node(layer);

        let result = service.verify(&config);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.message.contains("minzoom")));
    }

    #[test]
    fn test_verify_duplicate_node_ids() {
        let db = Database::new().unwrap();
        let service = ConfigService::new(db);

        let mut config = Config::new();
        let node1 = Node::new_resource(
            "test1".to_string(),
            None,
            vec!["test1.shp".to_string()],
            1000,
            "4326".to_string(),
            "table1".to_string(),
        );
        let mut node2 = Node::new_resource(
            "test2".to_string(),
            None,
            vec!["test2.shp".to_string()],
            1000,
            "4326".to_string(),
            "table2".to_string(),
        );
        node2.id = node1.id.clone(); // Duplicate ID

        config.add_node(node1);
        config.add_node(node2);

        let result = service.verify(&config);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.message.contains("Duplicate")));
    }
}
