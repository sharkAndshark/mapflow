# MapFlow

MapFlow是一个基于Rust语言编写的高性能地图瓦片服务器，采用可视化节点编辑器来发布地图服务。

## 特性

- **可视化节点编辑**：通过React前端界面进行节点拖拽和连接
- **多种节点类型**：支持resource、layer、xyz三种节点类型
- **Shapefile支持**：自动处理shapefile导入和坐标系统验证
- **DuckDB存储**：高性能的地理空间数据存储
- **实时验证**：配置验证和错误提示
- **一键部署**：验证通过后快速应用配置

## 技术栈

- **后端**：Rust + Axum
- **前端**：React + React Flow
- **数据库**：DuckDB with Spatial Extension
- **瓦片格式**：MVT (Mapbox Vector Tiles)

## 快速开始

### 环境要求

- Rust 1.80+
- Node.js 18+

### 安装和运行

```bash
# 克隆项目
git clone <repository-url>
cd mapflow

# 构建
cargo build --release

# 运行
PORT=3000 cargo run

# 访问
# Web界面: http://localhost:3000
# API文档: http://localhost:3000/config
```

## 使用流程

### 1. 上传数据文件

```bash
curl -X POST http://localhost:3000/upload \
  -F "file=@shapefile.zip"
```

系统会自动：
- 验证坐标系定义
- 导入数据到DuckDB
- 创建resource节点

### 2. 创建配置

通过Web界面或API创建layer和xyz节点，配置连接关系。

### 3. 验证和应用

```bash
# 验证配置
curl -X POST http://localhost:3000/verify \
  -H "Content-Type: application/json" \
  -d @config.json

# 应用配置
curl -X POST http://localhost:3000/config \
  -H "Content-Type: application/json" \
  -d @config.json
```

### 4. 获取瓦片

```bash
# 获取指定坐标的瓦片
curl http://localhost:3000/tiles/10/100/200.pbf --output tile.pbf
```

## API端点

### 文件上传

- `POST /upload` - 上传shapefile ZIP包
  - 参数：`file` (ZIP文件), `srid` (可选，坐标系ID)
  - 返回：创建的resource节点信息

### 配置管理

- `GET /config` - 获取当前配置
- `POST /config` - 应用新配置
- `POST /verify` - 验证配置

### 瓦片服务

- `GET /tiles/{z}/{x}/{y}.pbf` - 获取MVT格式瓦片
- `GET /` - Web界面
- `GET /resources/{id}/metadata` - 获取资源字段与边界信息

## 错误码

| 错误码 | 类型 | 说明 |
|--------|------|------|
| 4000 | 验证错误 | 配置验证失败 |
| 4004 | 资源不存在 | 请求的资源未找到 |
| 5000 | 数据库错误 | 数据库操作失败 |
| 5001 | IO错误 | 文件操作失败 |
| 5002 | 解析错误 | 数据解析失败 |
| 5003 | 配置错误 | 配置管理错误 |
| 5004 | 上传错误 | 文件上传失败 |
| 5005 | 瓦片生成错误 | 瓦片生成失败 |
| 5006 | 内部错误 | 服务器内部错误 |

## 配置格式

config.json包含nodes和edges数组：

```json
{
  "version": "0.1.0",
  "nodes": [
    {
      "id": "RES_XXX",
      "type": "resource",
      "name": "shanghai_buildings",
      "duckdb_table_name": "shanghai_XXX",
      "srid": "4326",
      "readonly": true
    },
    {
      "id": "LAYER_XXX",
      "type": "layer",
      "name": "buildings_layer",
      "source_resource_id": "RES_XXX",
      "minzoom": 10,
      "maxzoom": 18,
      "readonly": false
    },
    {
      "id": "XYZ_XXX",
      "type": "xyz",
      "name": "shanghai_tiles",
      "center": [121.4737, 31.2304, 12.0],
      "bounds": [-180.0, -85.0511, 180.0, 85.0511],
      "min_zoom": 0,
      "max_zoom": 22,
      "layers": [
        {
          "id": "layer_buildings",
          "source_layer_id": "LAYER_XXX"
        }
      ],
      "readonly": false
    }
  ],
  "edges": [
    {
      "id": "EDGE_1",
      "source": "RES_XXX",
      "target": "LAYER_XXX"
    },
    {
      "id": "EDGE_2",
      "source": "LAYER_XXX",
      "target": "XYZ_XXX"
    }
  ]
}
```

## 开发

### 运行测试

```bash
# 单元测试
cargo test

# 集成测试
cargo test --test integration

# 带输出的测试
cargo test -- --nocapture
```

### 准备测试数据

```bash
bash scripts/prepare_test_data.sh
```

这会下载OSM数据并转换为shapefile格式。

### 项目结构

```
mapflow/
├── src/
│   ├── main.rs          # 主程序和路由
│   ├── db.rs            # DuckDB连接管理
│   ├── models.rs        # 数据模型定义
│   └── services/        # 服务层
│       ├── config.rs    # 配置管理
│       ├── upload.rs    # 文件上传
│       └── tiles.rs     # 瓦片服务
├── tests/
│   └── integration/     # 集成测试
├── scripts/
│   └── prepare_test_data.sh
├── data/               # 上传文件存储
├── nodes.duckdb        # DuckDB数据库
└── config.json         # 配置文件
```

## License

MIT
