# MapFlow（探索版）

## 目标（先从最容易的开始）
让数据整理人员用**资源管理器式列表**上传并查看空间数据文件。

## 本阶段范围（最小可用）
- 仅支持**固定两种**空间数据格式的上传：`Shapefile`、`GeoJSON`（标准单文件，不支持按行分割/NDJSON）
  - `Shapefile` 必须为**zip 压缩包**，且包含同名的 `.shp` / `.shx` / `.dbf`（`.prj` 可缺省）
  - `GeoJSON` 仅接受 `.geojson` 文件扩展名
  - 坐标系不限制；若无法识别则在元数据中记录为 `crs: null`
- 单文件大小上限：默认 `200MB`，可通过 `UPLOAD_MAX_SIZE_MB` 配置
- 服务端口默认：`3000`
- 上传目录默认：`./uploads`
- 上传后在列表中展示文件（名称 / 类型 / 大小 / 时间）。
- 能浏览“我上传了哪些文件”。
- **地图预览（新开标签页）**：在列表中点击 Preview 新开标签页预览；后端支持 history fallback。

## 暂不做
- 服务发布
- 属性编辑
- 节点/流程

## 逐步探索计划（可随时迭代）
- Step 1: 上传 + 文件列表（完成）
- Step 2: 地图预览（新开标签页）（完成）
- Step 3: 文件详情侧栏（只读）（开发中）
- Step 4: 最小化发布入口（可选）

## 地图预览：最小接口（草案）
- `GET /api/files/:id/preview`
  - 返回：`id`, `name`, `crs`, `bbox`（[minx, miny, maxx, maxy]，WGS84，用于初始定位）
- `GET /api/files/:id/tiles/:z/:x/:y` (no .mvt extension)
  - 返回：`application/vnd.mapbox-vector-tile` 的二进制
  - 参数：`id` 为 files.id；z/x/y 为 WebMercator 瓦片坐标
  - 备注：当前瓦片不包含 `properties` 字段（仅几何），以保证性能和避免 NULL 类型问题。
- `GET /api/files/:id/feature/:fid` (计划中)
  - 返回：单个 Feature 的完整 GeoJSON（含属性）

## 地图预览：DuckDB Spatial 技术要点
- CRS 识别：用 `ST_Read_Meta(path)` 读取 `layers[1].geometry_fields[1].crs.auth_name/auth_code`，写入 `files.crs`（如 `EPSG:4326`）
- 重投影：用 `ST_Transform(geom, source_crs, 'EPSG:3857', always_xy := true|false)`
  - 注意：EPSG:4326 在 PROJ 定义里轴顺序是 [lat, lon]；GeoJSON 常见是 [lon, lat]，需要 `always_xy := true`
- 瓦片范围：`ST_TileEnvelope(z,x,y)` 返回 EPSG:3857 的 tile envelope
- MVT 编码：`ST_AsMVTGeom` 先裁剪/缩放到 tile extent，再 `ST_AsMVT` 输出 tile blob
- 参考文档（本机克隆）：`~/RiderProjects/duckdb-spatial/docs/functions.md`

## 最小界面（先落地最容易的）
- 顶部：`上传`按钮（选择文件上传）
- 列表列项：名称、类型、大小、上传时间、状态
- 状态值：上传中、已上传、失败
- 空状态：提示“暂未上传文件”
- 列表点击：仅高亮选中（不做详情面板）
- 本阶段使用**表格视图**；后期可选添加图标视图切换

## 最小接口（仅覆盖本阶段）
- `POST /api/uploads`
  - 表单字段：`file`
  - 仅接受 `zip`（Shapefile）或 `.geojson`（单文件）
  - 失败返回清晰错误（格式不支持 / 超过配置上限）
  - 成功返回：新文件的元数据（同 `GET /api/files` 单条结构）
- `GET /api/files`
  - 返回列表字段：`id`、`name`、`type`、`size`、`uploadedAt`、`status`、`crs`

## 最小存储约定（文件系统）
- 每个上传文件保存到：`./uploads/<id>/`
- 列表元数据保存到：`./uploads/index.json`
- 元数据字段（示例）：`id`、`name`、`type`、`size`、`uploadedAt`、`status`、`crs`、`path`、`error`
- `crs`：若识别不到坐标系，则为 `null`
- 状态流转：
  - 上传开始：`uploading`（可选）
  - 上传成功：`uploaded`
  - 上传失败：`failed` + `error`
- 服务重启：将残留 `uploading` 视为 `failed`（保证一致）

示例 `./uploads/index.json`：
```json
[
  {
    "id": "f1a2b3",
    "name": "roads",
    "type": "shapefile",
    "size": 12582912,
    "uploadedAt": "2026-02-04T10:21:00Z",
    "status": "uploaded",
    "crs": null,
    "path": "./uploads/f1a2b3/roads.zip"
  },
  {
    "id": "c9d8e7",
    "name": "parks",
    "type": "geojson",
    "size": 2097152,
    "uploadedAt": "2026-02-04T10:30:00Z",
    "status": "failed",
    "crs": null,
    "path": "./uploads/c9d8e7/parks.geojson",
    "error": "Invalid GeoJSON"
  }
]
```

## 本地开发
后端使用 Rust (axum)，前端使用 React + Vite。

### 环境变量配置

#### 后端配置
- `PORT`：服务端口，默认 `3000`
- `UPLOAD_MAX_SIZE_MB`：单文件大小上限（单位 MB，正整数），默认 `200`
- `UPLOAD_DIR`：上传目录，默认 `./uploads`
- `DB_PATH`：DuckDB 数据库文件路径，默认 `./data/mapflow.duckdb`

#### 前端开发配置
- `VITE_PORT`：前端开发服务器端口，默认 `5173`

#### 测试配置
- `TEST_PORT`：E2E 测试使用的端口，默认 `3000`

### 使用 .env 文件

1. 复制配置模板：`cp .env.example .env`
2. 修改 `.env` 文件中的配置
3. 启动服务（会自动读取 .env 文件）

示例（仅当前命令生效）：
```bash
# 后端
UPLOAD_MAX_SIZE_MB=50 cargo run --manifest-path backend/Cargo.toml

# 前端
VITE_PORT=3001 npm run dev

# 测试
TEST_PORT=9999 npm run test:e2e
```

### 避免端口冲突

如需同时运行开发服务器和测试，可使用不同端口：

```bash
# 终端 1：开发服务器使用 8080 端口
PORT=8080 cargo run --manifest-path backend/Cargo.toml

# 终端 2：E2E 测试使用 9999 端口（避免冲突）
TEST_PORT=9999 npm run test:e2e
```

### 可选：使用 justfile
如果你安装了 `just`，可以用以下命令快速编排：
- `just start` 或 `just dev`：同时启动前后端（最快开发模式）
- `just docker-up`：Docker 启动（不重新构建）
- `just docker-up-build`：Docker 重新构建并启动
- `just build`：本地打包前端 + 构建后端
- `just check`：运行 fmt 和 clippy 检查
- `just test`：全量测试

### 安装依赖
- `just install`：安装前端依赖
- 后端依赖会在首次运行 cargo 时自动下载

### 启动开发环境
1. 快速启动：`just start`
   - 系统会自动选择空闲端口（避免冲突）
   - **请查看终端输出**获取实际访问地址（例如 `Frontend will run at: http://localhost:xxxx`）
2. 后端 API 地址也会在终端显示

### 生产/测试（后端直出前端静态资源）
1. 构建前端：`cd frontend && npm run build`
2. 启动后端：`cargo run --manifest-path backend/Cargo.toml`
3. 访问 `http://localhost:3000`

### 行为测试（Playwright）
1. 安装浏览器：`cd frontend && npx playwright install`
2. 运行测试：`cd frontend && npm run test:e2e`

## 行为测试清单
行为测试的清单与约束记录在 `TESTS.md`，用于保持对外可观测行为的稳定契约。

## 需要你确定的小点
- 这一阶段不再扩展格式，先把上传与列表流程做顺。
