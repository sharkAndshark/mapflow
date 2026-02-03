pub mod error;
pub use error::{AppError, Result};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Resource,
    Layer,
    Xyz,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub readonly: bool,

    #[serde(flatten)]
    pub data: NodeData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NodeData {
    Resource(ResourceData),
    Layer(LayerData),
    Xyz(XyzData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceData {
    pub resource_type: String,
    pub file_path: Vec<String>,
    pub size: u64,
    pub create_timestamp: i64,
    pub hash: String,
    pub srid: String,
    pub duckdb_table_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerData {
    pub source_resource_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    pub minzoom: u32,
    pub maxzoom: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XyzLayerRef {
    pub id: String,
    pub source_layer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XyzData {
    pub center: Vec<f64>,
    pub min_zoom: u32,
    pub max_zoom: u32,
    pub fillzoom: u32,
    pub bounds: Vec<f64>,
    pub layers: Vec<XyzLayerRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Config {
    pub fn new() -> Self {
        Self {
            version: "0.1.0".to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn find_node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn find_node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.push(node);
    }

    pub fn remove_node(&mut self, id: &str) {
        self.nodes.retain(|n| n.id != id);
        self.edges.retain(|e| e.source != id && e.target != id);
    }

    pub fn add_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    pub fn remove_edge(&mut self, id: &str) {
        self.edges.retain(|e| e.id != id);
    }
}

impl Node {
    pub fn new_resource(
        name: String,
        description: Option<String>,
        file_path: Vec<String>,
        size: u64,
        srid: String,
        duckdb_table_name: String,
    ) -> Self {
        let hash = format!(
            "{:x}",
            xxhash_rust::xxh64::xxh64(file_path.join(",").as_bytes(), 0)
        );
        let create_timestamp = Utc::now().timestamp();
        let id = format!("RES_{:X}", Uuid::new_v4());

        Self {
            id,
            node_type: NodeType::Resource,
            name,
            description,
            readonly: true,
            data: NodeData::Resource(ResourceData {
                resource_type: "shapefile".to_string(),
                file_path,
                size,
                create_timestamp,
                hash,
                srid,
                duckdb_table_name,
            }),
        }
    }

    pub fn new_layer(
        name: String,
        description: Option<String>,
        source_resource_id: String,
        fields: Option<Vec<String>>,
        minzoom: u32,
        maxzoom: u32,
    ) -> Self {
        let id = format!("LAYER_{:X}", Uuid::new_v4());

        Self {
            id,
            node_type: NodeType::Layer,
            name,
            description,
            readonly: false,
            data: NodeData::Layer(LayerData {
                source_resource_id,
                fields,
                minzoom,
                maxzoom,
            }),
        }
    }

    pub fn new_xyz(
        name: String,
        description: Option<String>,
        center: Vec<f64>,
        min_zoom: u32,
        max_zoom: u32,
        fillzoom: u32,
        bounds: Vec<f64>,
        layers: Vec<XyzLayerRef>,
    ) -> Self {
        let id = format!("XYZ_{:X}", Uuid::new_v4());

        Self {
            id,
            node_type: NodeType::Xyz,
            name,
            description,
            readonly: false,
            data: NodeData::Xyz(XyzData {
                center,
                min_zoom,
                max_zoom,
                fillzoom,
                bounds,
                layers,
            }),
        }
    }

    pub fn get_resource_data(&self) -> Option<&ResourceData> {
        match &self.data {
            NodeData::Resource(data) => Some(data),
            _ => None,
        }
    }

    pub fn get_layer_data(&self) -> Option<&LayerData> {
        match &self.data {
            NodeData::Layer(data) => Some(data),
            _ => None,
        }
    }

    pub fn get_xyz_data(&self) -> Option<&XyzData> {
        match &self.data {
            NodeData::Xyz(data) => Some(data),
            _ => None,
        }
    }

    pub fn data_matches_type(&self) -> bool {
        matches!(
            (&self.node_type, &self.data),
            (NodeType::Resource, NodeData::Resource(_))
                | (NodeType::Layer, NodeData::Layer(_))
                | (NodeType::Xyz, NodeData::Xyz(_))
        )
    }
}

impl Edge {
    pub fn new(source: String, target: String) -> Self {
        let id = format!("EDGE_{:X}", Uuid::new_v4());
        Self { id, source, target }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_crud() {
        let mut config = Config::new();
        assert_eq!(config.nodes.len(), 0);

        let node = Node::new_resource(
            "test".to_string(),
            None,
            vec!["test.shp".to_string()],
            1000,
            "4326".to_string(),
            "test_table".to_string(),
        );
        config.add_node(node.clone());
        assert_eq!(config.nodes.len(), 1);

        let found = config.find_node(&node.id);
        assert!(found.is_some());

        config.remove_node(&node.id);
        assert_eq!(config.nodes.len(), 0);
    }

    #[test]
    fn test_edge_crud() {
        let mut config = Config::new();
        let edge = Edge::new("node1".to_string(), "node2".to_string());
        config.add_edge(edge.clone());
        assert_eq!(config.edges.len(), 1);

        config.remove_edge(&edge.id);
        assert_eq!(config.edges.len(), 0);
    }
}
