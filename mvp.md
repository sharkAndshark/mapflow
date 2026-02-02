# MapNodes

Publishing your tiles service with simply dragging! MapNodes是一个rust语言编写的高性能地图服务器，他支持用户通过简单的拖拽节点，连接节点来发布自己的地图服务。

## 目录

- [项目概述](#项目概述)
- [系统架构](#系统架构)
- [安装与设置](#安装与设置)
- [API文档](#api文档)
- [配置格式](#配置格式)
- [使用流程](#使用流程)
- [核心功能与行为](#核心功能与行为)
- [参考链接](#参考链接)

## 项目概述

MapNodes是一个基于Rust语言编写的高性能地图瓦片服务器，采用节点拖拽的可视化方式来发布地图服务。用户可以通过简单的拖拽和连接节点来创建复杂的地图瓦片服务，无需编写复杂的配置文件。

### 核心特性

- **可视化节点编辑**：通过React前端界面进行节点拖拽和连接
- **多种节点类型**：支持resource、layer、xyz三种节点类型
- **Shapefile支持**：自动处理shapefile导入和坐标系统验证
- **DuckDB存储**：高性能的地理空间数据存储
- **实时验证**：配置验证和错误提示
- **一键部署**：验证通过后快速应用配置

### 使用场景

- 快速发布地图瓦片服务
- 地理数据的可视化呈现
- 多图层地图服务
- POI数据瓦片化
- 自定义地图服务开发

## 系统架构

### 节点类型

系统采用基于节点的架构设计，包含三种主要节点类型：

#### 1. Resource节点（资源节点）
- **类型**：`resource`
- **用途**：表示地理数据源，通常来自上传的shapefile文件
- **特性**：
  - 只读节点，由上传shapefile自动创建
  - 包含文件元数据（名称、大小、时间戳、哈希值）
  - 支持坐标系定义（SRID）
  - 自动导入到DuckDB表

#### 2. Layer节点（图层节点）
- **类型**：`layer`
- **用途**：基于resource节点创建的可配置图层
- **特性**：
  - 可编辑节点
  - 字段选择功能
  - 缩放级别设置（minzoom、maxzoom）
  - 连接到特定的resource节点

#### 3. XYZ节点（瓦片节点）
- **类型**：`xyz`
- **用途**：聚合多个layer节点，生成最终的XYZ瓦片服务
- **特性**：
  - 可编辑节点
  - 定义瓦片服务的全局参数（中心点、边界、缩放级别）
  - 包含多个图层引用
  - 提供瓦片服务端点

### 数据流程

```
Shapefile上传 → Resource节点创建 → DuckDB导入
                          ↓
                    Layer节点配置
                          ↓
                    XYZ节点聚合
                          ↓
                    XYZ瓦片服务
```

### 技术栈

- **后端**：Rust
- **前端**：React
- **数据存储**：DuckDB with Spatial Extension
- **数据格式**：Shapefile (ZIP格式)
- **瓦片格式**：XYZ tiles
- **坐标系统**：支持多种SRID

## 安装与设置

### 系统要求

- 操作系统：支持Rust的平台（Linux、macOS、Windows）
- Rust版本：最新稳定版
- 内存：建议4GB以上
- 磁盘空间：根据数据量确定

### 安装步骤

1. **克隆或下载项目**

2. **构建项目**
   ```bash
   cargo build --release
   ```

3. **运行服务器**
   ```bash
   cargo run
   ```

4. **访问系统**
   打开浏览器访问 `http://localhost:3000`

### 目录结构

```
mapflow/
├── nodes.duckdb        # DuckDB数据库文件（自动创建）
├── config.json         # 配置文件（可选）
├── data/              # 上传文件存储目录（自动创建）
└── mvp.md             # 本文档
```

### 初始化行为

系统启动时会自动执行以下初始化操作：

1. **端口监听**：自动监听3000端口
2. **数据库初始化**：
   - 查找`nodes.duckdb`文件
   - 若不存在则创建并加载spatial extension
3. **数据目录初始化**：
   - 查找`data`文件夹
   - 若不存在则创建，用于保存上传的文件
4. **配置文件检查**：
   - 查找`config.json`文件
   - 若不存在，不创建，仅在控制台提示

## API文档

### 文件上传API

#### 上传Shapefile
- **端点**：`POST /upload`
- **支持格式**：shapefile ZIP包
- **参数**：
  - `file`: shapefile压缩包
  - `srid` (可选): 坐标系ID，用于处理缺少.prj文件的情况
- **行为**：
  - 验证坐标系定义是否有效
  - 若无有效坐标系，返回HTTP 400错误
  - 导入数据到DuckDB表，表名格式：`文件名_guid`
  - 自动创建对应的resource节点
- **错误处理**：
  - 400: 坐标系定义无效
  - 415: 不支持的文件格式

### 配置管理API

#### 获取配置
- **端点**：`GET /config`
- **描述**：获取当前的config.json配置
- **返回**：JSON格式的配置对象

#### 应用配置
- **端点**：`POST /config`
- **描述**：应用新的配置文件
- **行为**：
  - 验证配置的完整性和正确性
  - 若验证通过，覆盖本地config.json文件
  - 重启服务器以应用新配置
- **错误处理**：
  - 400: 配置验证失败
  - 500: 服务器重启失败

#### 验证配置
- **端点**：`POST /verify`
- **描述**：验证提供的配置文件，帮助前端用户检验拖拽配置
- **行为**：
  - 执行完整的配置验证逻辑
  - 检查节点连接的完整性
  - 验证节点参数的有效性
- **返回**：验证结果和错误信息（如果有）

### 前端API

#### 系统首页
- **端点**：`GET /`
- **描述**：系统首页，提供React编写的可视化编辑界面
- **行为**：
  - 自动调用get config接口获取当前配置
  - 渲染节点连接的网络结构
  - 提供节点编辑和连接功能
  - 显示验证和应用按钮

#### 瓦片服务
- **端点**：`GET /tiles/{z}/{x}/{y}.pbf`
- **描述**：获取指定坐标的瓦片数据
- **参数**：
  - `z`: 缩放级别
  - `x`: 瓦片X坐标
  - `y`: 瓦片Y坐标
- **返回**：Protocol Buffer格式的瓦片数据

## 配置格式

### config.json结构

```jsonc
{
    "version": "0.1.0",
    "nodes": [
        {
            "id": "RES_3E8F1D0220E9B0EE50D4347A46D18CBD",
            "type": "resource",
            "resource_type": "shapefile",
            "name": "shanghai_buildings",
            "description": "shanghai buildings",
            "file_path": ["/path/to/shanghai_buildings.shp"],
            "size": 9999,
            "create_timestamp": 1769874490,
            "hash": "xxhash_value",
            "srid": "4326",
            "duckdb_table_name": "shanghai_EB24274F4992232980E515AD2F977EFA",
            "readonly": true
        },
        {
            "id": "RES_7297D7AEB16ADE5D9B6C478128BA3D27",
            "type": "resource",
            "resource_type": "shapefile",
            "name": "shanghai_coffees",
            "description": "shanghai coffees",
            "file_path": ["/path/to/shanghai_coffees.shp"],
            "size": 8888,
            "create_timestamp": 1769871490,
            "hash": "xxhash_value",
            "srid": "4326",
            "duckdb_table_name": "shanghai_coffee_EB24274F4992232980E515AD2F977EFA",
            "readonly": true
        },
        {
            "id": "LAYER_A1B2C3D4E5F6",
            "type": "layer",
            "source_resource_id": "RES_3E8F1D0220E9B0EE50D4347A46D18CBD",
            "name": "buildings_layer",
            "description": "building layer",
            "fields": ["field1", "field2"],
            "minzoom": 8,
            "maxzoom": 14,
            "readonly": false
        },
        {
            "id": "LAYER_X9Y8Z7W6V5U4",
            "type": "layer",
            "source_resource_id": "RES_7297D7AEB16ADE5D9B6C478128BA3D27",
            "name": "coffees_layer",
            "description": "coffee layer",
            "fields": ["field1", "field2", "field3"],
            "minzoom": 10,
            "maxzoom": 18,
            "readonly": false
        },
        {
            "id": "XYZ_1234567890ABC",
            "type": "xyz",
            "name": "shanghai_pois",
            "description": "shanghai POI tiles",
            "center": [-76.275, 39.153, 8],
            "min_zoom": 0,
            "max_zoom": 22,
            "fillzoom": 12,
            "bounds": [-180, -85.0511, 180, 85.0511],
            "readonly": false,
            "layers": [
                {
                    "id": "layer_buildings",
                    "source_layer_id": "LAYER_A1B2C3D4E5F6"
                },
                {
                    "id": "layer_coffees",
                    "source_layer_id": "LAYER_X9Y8Z7W6V5U4"
                }
            ]
        }
    ],
    "edges": [
        {
            "id": "EDGE_1",
            "source": "RES_3E8F1D0220E9B0EE50D4347A46D18CBD",
            "target": "LAYER_A1B2C3D4E5F6"
        },
        {
            "id": "EDGE_2",
            "source": "RES_7297D7AEB16ADE5D9B6C478128BA3D27",
            "target": "LAYER_X9Y8Z7W6V5U4"
        },
        {
            "id": "EDGE_3",
            "source": "LAYER_A1B2C3D4E5F6",
            "target": "XYZ_1234567890ABC"
        },
        {
            "id": "EDGE_4",
            "source": "LAYER_X9Y8Z7W6V5U4",
            "target": "XYZ_1234567890ABC"
        }
    ]
}
```

### 配置属性说明

#### Resource节点属性

| 属性 | 类型 | 必需 | 描述 |
|------|------|------|------|
| id | string | 是 | 节点唯一标识符 |
| type | string | 是 | 节点类型，值为"resource" |
| resource_type | string | 是 | 资源类型，目前支持"shapefile" |
| name | string | 是 | 资源名称 |
| description | string | 否 | 资源描述 |
| file_path | array | 是 | 文件路径数组 |
| size | number | 是 | 文件大小（字节） |
| create_timestamp | number | 是 | 创建时间戳 |
| hash | string | 是 | 文件哈希值 |
| srid | string | 是 | 坐标系ID |
| duckdb_table_name | string | 是 | DuckDB中的表名 |
| readonly | boolean | 是 | 是否只读，resource节点必须为true |

#### Layer节点属性

| 属性 | 类型 | 必需 | 描述 |
|------|------|------|------|
| id | string | 是 | 节点唯一标识符 |
| type | string | 是 | 节点类型，值为"layer" |
| source_resource_id | string | 是 | 源resource节点ID |
| name | string | 是 | 图层名称 |
| description | string | 否 | 图层描述 |
| fields | array | 否 | 要包含的字段列表 |
| minzoom | number | 是 | 最小缩放级别 |
| maxzoom | number | 是 | 最大缩放级别 |
| readonly | boolean | 是 | 是否只读，layer节点通常为false |

#### XYZ节点属性

| 属性 | 类型 | 必需 | 描述 |
|------|------|------|------|
| id | string | 是 | 节点唯一标识符 |
| type | string | 是 | 节点类型，值为"xyz" |
| name | string | 是 | XYZ服务名称 |
| description | string | 否 | 服务描述 |
| center | array | 是 | 中心点坐标 [经度, 纬度, 缩放级别] |
| min_zoom | number | 是 | 最小缩放级别 |
| max_zoom | number | 是 | 最大缩放级别 |
| fillzoom | number | 是 | 填充缩放级别 |
| bounds | array | 是 | 边界 [最小经度, 最小纬度, 最大经度, 最大纬度] |
| readonly | boolean | 是 | 是否只读，xyz节点通常为false |
| layers | array | 是 | 包含的layer引用数组 |

#### Edge属性

| 属性 | 类型 | 必需 | 描述 |
|------|------|------|------|
| id | string | 是 | 边唯一标识符 |
| source | string | 是 | 源节点ID |
| target | string | 是 | 目标节点ID |

## 使用流程

### 基本工作流程

#### 1. 上传数据文件
1. 访问系统首页 `http://localhost:3000`
2. 使用上传接口上传shapefile ZIP包
3. 系统自动创建resource节点
4. 验证坐标系定义，处理坐标转换

#### 2. 创建Layer节点
1. 在首页右键单击，选择创建layer节点
2. 配置layer节点属性：
   - 选择源resource节点
   - 设置图层名称和描述
   - 选择要包含的字段
   - 设置缩放级别范围（minzoom、maxzoom）
3. 创建资源到layer的连接

#### 3. 创建XYZ节点
1. 在首页右键单击，选择创建xyz节点
2. 配置xyz节点属性：
   - 设置服务名称和描述
   - 定义中心点和边界
   - 设置缩放级别参数
3. 添加需要包含的layer节点
4. 创建layer到xyz的连接

#### 4. 验证配置
1. 点击首页的"validate"按钮
2. 系统验证当前节点网络的完整性
3. 检查节点连接是否正确
4. 验证参数设置是否有效
5. 修复任何验证错误

#### 5. 应用配置
1. 验证通过后，点击"apply"按钮
2. 系统应用当前配置
3. 服务器自动重启
4. XYZ瓦片服务开始运行

### 使用示例

#### 示例1：简单POI地图服务

**目标**：创建一个显示上海咖啡店的瓦片服务

**步骤**：
1. 上传包含上海咖啡店位置数据的shapefile
   - 系统创建resource节点：`RES_coffee_shanghai`
   - 数据导入DuckDB表：`coffee_EB24274F4992232980E515AD2F977EFA`

2. 创建layer节点
   - 连接到`RES_coffee_shanghai`
   - 选择字段：`name`, `address`, `rating`
   - 设置缩放级别：10-18
   - 节点ID：`LAYER_coffee`

3. 创建xyz节点
   - 服务名称：`shanghai_coffees`
   - 中心点：`[121.4737, 31.2304, 12]`
   - 添加layer：`LAYER_coffee`
   - 节点ID：`XYZ_shanghai_coffee`

4. 验证并应用配置

#### 示例2：多图层地图服务

**目标**：创建包含建筑物和咖啡店的复合地图服务

**步骤**：
1. 上传两个shapefile：
   - 上海建筑物数据
   - 上海咖啡店数据

2. 创建两个layer节点：
   - `LAYER_buildings`：连接建筑物数据，缩放8-14
   - `LAYER_coffees`：连接咖啡店数据，缩放10-18

3. 创建xyz节点：
   - 服务名称：`shanghai_pois`
   - 包含两个layer节点
   - 设置适当的缩放级别范围

4. 验证并应用配置

### 常见操作

#### 右键菜单操作
- 右键单击画布空白处：
  - 新增xyz节点
  - 新增layer节点

#### 节点编辑
- 单击节点：选中节点
- 拖拽节点：移动节点位置
- 双击节点：编辑节点属性
- Delete键：删除选中节点

#### 连接操作
- 拖拽源节点到目标节点：创建连接
- 双击连接：编辑连接属性
- Delete键：删除选中连接

## 核心功能与行为

### 服务器启动行为

1. **端口监听**
   - 自动监听3000端口
   - 提供HTTP服务

2. **数据库初始化**
   - 查找`nodes.duckdb`文件
   - 若不存在则创建
   - 加载spatial extension

3. **数据目录设置**
   - 查找`data`文件夹
   - 若不存在则创建
   - 用于保存上传的文件

4. **配置文件检查**
   - 查找`config.json`文件
   - 若不存在，不创建
   - 在控制台提示未找到配置文件

### 文件处理行为

1. **文件上传验证**
   - 检查文件格式（目前只支持shapefile ZIP）
   - 验证坐标系定义
   - 若无有效坐标系，返回HTTP 400错误

2. **数据导入**
   - 解析shapefile
   - 导入到DuckDB表
   - 表名格式：`文件名_guid`
   - 创建对应的resource节点

3. **坐标系统处理**
   - 支持可选的srid参数
   - 处理缺少.prj文件的情况
   - 验证坐标系有效性

### 配置管理行为

1. **配置获取**
   - 读取本地config.json
   - 返回当前配置状态

2. **配置应用**
   - 验证配置完整性
   - 覆盖本地配置文件
   - 重启服务器

3. **配置验证**
   - 检查节点连接完整性
   - 验证节点参数有效性
   - 返回验证结果和错误信息

### 前端交互行为

1. **首页功能**
   - React编写的前端界面
   - 可视化节点编辑器
   - 实时渲染节点网络

2. **节点操作**
   - 上传shapefile创建resource节点（只读）
   - 右键单击创建xyz或layer节点
   - 拖拽连接节点

3. **验证和应用**
   - validate按钮：验证当前配置
   - apply按钮：应用当前配置

4. **节点类型特性**
   - resource节点：只读，由上传创建
   - layer节点：可编辑，字段选择和缩放设置
   - xyz节点：可编辑，聚合layers

## 参考链接

### 技术文档

1. [DuckDB Rust Crate](https://crates.io/crates/duckdb)
   - DuckDB Rust绑定文档
   - 构建优化参考（如果构建过慢）

2. [DuckDB Spatial Extension](https://duckdb.org/docs/extensions/spatial)
   - 地理空间扩展文档
   - 空间函数参考

3. [Mapbox Vector Tiles规范](https://docs.mapbox.com/vector-tiles/specification/)
   - 瓦片数据格式规范

### 开发资源

1. [Rust官方文档](https://doc.rust-lang.org/)
2. [React官方文档](https://react.dev/)
3. [Shapefile格式规范](https://www.esri.com/content/dam/esrisites/sitecontent-archive/pdfs/technical-library/shapefile.pdf)

---

**版本**：0.1.0  
**最后更新**：2026-02-02