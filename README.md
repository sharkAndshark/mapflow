# MapFlow（探索版）

## 目标（先从最容易的开始）
让数据整理人员用**资源管理器式列表**上传并查看空间数据文件。

## 本阶段范围（最小可用）
- 仅支持**固定两种**空间数据格式的上传：`Shapefile`、`GeoJSON`（标准单文件，不支持按行分割/NDJSON）
  - `Shapefile` 必须为**zip 压缩包**，且包含同名的 `.shp` / `.shx` / `.dbf`（`.prj` 可缺省）
  - `GeoJSON` 仅接受 `.geojson` 文件扩展名
  - 坐标系不限制；若无法识别则在元数据中记录为 `crs: null`
- 单文件大小上限：`200MB`（硬限制）
- 服务端口默认：`3000`
- 上传目录默认：`./uploads`
- 上传后在列表中展示文件（名称 / 类型 / 大小 / 时间）。
- 能浏览“我上传了哪些文件”。

## 暂不做
- 服务发布
- 属性编辑
- 地图预览
- 节点/流程

## 逐步探索计划（可随时迭代）
- Step 1: 上传 + 文件列表（完成本阶段）
- Step 2: 文件详情侧栏（只读）
- Step 3: 最小化发布入口（可选）

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
  - 失败返回清晰错误（格式不支持 / 超过 200MB）
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

### 可选：使用 justfile
如果你安装了 `just`，可以用以下命令快速编排：
- `just dev`：启动前后端（提示两个终端命令）
- `just build`：前端打包 + 后端构建
- `just backend-test`：后端测试
- `just e2e`：前端行为测试
- `just test`：全量测试

### 安装依赖
- 后端：`cargo build --manifest-path backend/Cargo.toml`
- 前端：`cd frontend && npm install`

### 启动开发环境
1. 启动后端：`cargo run --manifest-path backend/Cargo.toml`
2. 启动前端：`cd frontend && npm run dev`
3. 打开 `http://localhost:5173`

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
