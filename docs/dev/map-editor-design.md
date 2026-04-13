# 地图编辑器架构设计

> **状态**：设计阶段，未实施
> **创建**：2026-04-12
> **关联**：`docs/internal.md` § 地图编辑器

## 决策记录

| # | 决策 | 结论 | 理由 |
|---|------|------|------|
| 1 | 发布架构 | 统一到地图编辑器，数据/资源只做 CRUD + 预览 | 职责清晰，消除"数据界面也能发布"的混乱 |
| 2 | 存储格式 | 标准 Mapbox Style Spec v8 JSON | 不锁死自己；standard CRS 场景可直接导出给 MapLibre/Mapbox 消费 |
| 3 | 渲染引擎 | OpenLayers（Flat Style + Expression API） | Custom CRS 原生支持；已有完整 OL 渲染管线 |
| 4 | Custom CRS | 通过 `_mapflow:*` 扩展字段在 style JSON 中表达 | Mapbox Style Spec 允许自定义扩展；OL 已有自定义 Projection + TileGrid 能力 |
| 5 | 编辑器 UX | ArcGIS 风格符号化向导 | 制图人员熟悉 ArcGIS 心智模型（单一颜色/分类/分级），Maputnik 属性面板推广失败 |
| 6 | 首页结构 | `[数据] [资源] [地图]` 三 Tab 并列 | 地图是独立概念，与数据/资源同级 |
| 7 | 旧 URL | 不兼容，直接废弃 | 软件未发布，无存量用户依赖 |
| 8 | 迁移策略 | 渐进式四阶段 | 降低风险，每阶段可独立交付 |

## 架构总览

```
用户层:
  数据面板(CRUD+预览)  资源面板(CRUD)  地图面板(列表+发布)
                                           ↓
                                      MapEditor (/editor/:id)
                                      ┌──────────────────┐
                                      │ ArcGIS风格向导UX   │
                                      │ ↓ 生成/编辑       │
                                      │ Mapbox Style JSON │
                                      └───────┬──────────┘
                                              │
技术层:                        ┌───────────────┼───────────────┐
                               ↓               ↓               ↓
                        编辑器预览        公开查看器         外部导出
                        转换层→OL         转换层→OL        直接输出
                        (custom CRS ✓)   (custom CRS ✓)   style.json
                                                          (standard CRS)
存储层:
  maps.style_json → files (数据源引用)
                  → fonts (字体引用)
                  → icons (图标引用, sprite生成)
```

## 数据模型

### 新增 `maps` 表

```sql
CREATE TABLE maps (
    id VARCHAR PRIMARY KEY,
    name VARCHAR NOT NULL,
    workspace_id VARCHAR NOT NULL,
    style_json VARCHAR,
    slug VARCHAR UNIQUE,
    is_public BOOLEAN DEFAULT FALSE,
    published_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id)
);

CREATE INDEX idx_maps_workspace ON maps(workspace_id);
```

### 引用关系（逻辑，非外键）

```
maps.style_json → files.id     (source 的 tiles URL 中编码)
maps.style_json → fonts        (layer 的 text-font 属性)
maps.style_json → icons        (layer 的 icon-image + sprite)
```

## API 设计

### 地图管理（需认证）

| 方法 | 路径 | 用途 |
|------|------|------|
| GET | `/api/maps` | 列表（workspace 过滤） |
| POST | `/api/maps` | 创建 `{name}` |
| GET | `/api/maps/{id}` | 详情（含 style JSON） |
| PUT | `/api/maps/{id}` | 更新（name / style JSON） |
| DELETE | `/api/maps/{id}` | 删除 |
| POST | `/api/maps/{id}/publish` | 发布 `{slug?}` |
| POST | `/api/maps/{id}/unpublish` | 取消发布 |
| GET | `/api/maps/{id}/preview-sources` | 可用数据源列表（编辑器用） |

### 公开端点（无需认证）

| 方法 | 路径 | 用途 |
|------|------|------|
| GET | `/maps/{slug}/style.json` | 完整 style JSON |
| GET | `/maps/{slug}/tiles/{sourceId}/{z}/{x}/{y}` | 瓦片代理 |
| GET | `/maps/{slug}/fonts/{fontstack}/{range}` | 字体字形代理 |
| GET | `/maps/{slug}/sprite/sprite.json` | Sprite 索引 |
| GET | `/maps/{slug}/sprite/sprite.png` | Sprite 图集 |
| GET | `/maps/{slug}/meta` | 元数据 |
| GET | `/maps/{slug}/embed` | 嵌入式查看器 |

### 将被移除的端点（Phase 4）

- `/api/files/{id}/publish`、`/api/files/{id}/unpublish`、`/api/files/{id}/public-url`、`/api/files/{id}/publish-settings`、`/api/files/{id}/zoom`
- `/api/fonts/{id}/publish`、`/api/fonts/{id}/unpublish`
- `/tiles/{slug}/{z}/{x}/{y}`、`/tiles/{slug}`、`/tiles/{slug}/meta`
- `/fonts/{workspaceSlug}/{fontstack}/{range}`

## 转换层：Mapbox Style JSON → OL Flat Style

### 核心发现

OL Expression API 与 Mapbox 表达式几乎 1:1 对应。转换层的核心工作是属性名映射（约 30 个）和少量语义差异处理，表达式层面几乎直传。

### 属性映射

**fill (polygon)**

| Mapbox | OL Flat | 转换逻辑 |
|--------|---------|---------|
| `fill-color` | `fill-color` | 直传 |
| `fill-opacity` | 合并到 `fill-color` alpha | 需表达式合并 |
| `fill-outline-color` | `stroke-color` | polygon 需额外 stroke |

**line**

| Mapbox | OL Flat | 转换逻辑 |
|--------|---------|---------|
| `line-color` | `stroke-color` | 直传 |
| `line-width` | `stroke-width` | 直传 |
| `line-opacity` | 合并到 `stroke-color` alpha | 需表达式合并 |
| `line-dasharray` | `stroke-line-dash` | 直传 |
| `line-cap` | `stroke-line-cap` | 直传 |
| `line-join` | `stroke-line-join` | 直传 |

**circle (point)**

| Mapbox | OL Flat | 转换逻辑 |
|--------|---------|---------|
| `circle-radius` | `circle-radius` | 直传 |
| `circle-color` | `circle-fill-color` | 名称映射 |
| `circle-opacity` | `circle-opacity` | 直传 |
| `circle-stroke-color` | `circle-stroke-color` | 直传 |
| `circle-stroke-width` | `circle-stroke-width` | 直传 |

**symbol (label + icon)**

| Mapbox | OL Flat | 转换逻辑 |
|--------|---------|---------|
| `text-field` | `text-value` | 名称映射，`{property}` → 表达式 |
| `text-font` | `text-font` | CSS font 字符串合并 |
| `text-size` | 合并到 `text-font` | `16px` → font-size 部分 |
| `text-color` | `text-fill-color` | 名称映射 |
| `text-halo-color` | `text-stroke-color` | 名称映射 |
| `text-halo-width` | `text-stroke-width` | 名称映射 |
| `icon-image` | `icon-src` | sprite 名称 → URL |
| `icon-size` | `icon-scale` | 名称映射 |

### 表达式映射

**几乎直传**（语法一致）：`['get','x']`、`['==',a,b]`、`['>',a,b]`、`['all',...]`、`['any',...]`、`['case',...]`、`['match',...]`、`['interpolate',['linear'],...]`、`['coalesce',...]`

**需转换**：

| Mapbox | OL | 说明 |
|--------|-----|------|
| `['zoom']` | `['resolution']` + 换算 | zoom↔resolution 是幂函数 |
| `['step',...]` | `['case',...]` 重写 | OL 无 step |
| `['let',...]/['var',...]` | `['var',...]` + variables | 需提取变量 |

### Phase 1 不支持（降级为默认样式）

| 类型 | 原因 |
|------|------|
| `fill-extrusion` | OL 不支持 3D |
| `heatmap` | OL 需单独 layer type |
| `hillshade` | 栅格处理，后续迭代 |
| `line-gradient` | OL 不原生支持 |
| data-driven `icon-image` | sprite 动态映射复杂度高 |

## Custom CRS 处理

### 现有能力（不需要改动）

- 后端 `calculate_custom_tile_bbox()` — 原始坐标空间裁切瓦片
- OL 自定义 `Projection` + `TileGrid` — 渲染原始坐标瓦片
- 后端 `data_bounds` — 数据空间范围

### Style JSON 中表达 Custom CRS Source

```json
{
  "sources": {
    "my-building": {
      "type": "vector",
      "tiles": ["/maps/{slug}/tiles/my-building/{z}/{x}/{y}"],
      "minzoom": 0,
      "maxzoom": 20,
      "_mapflow:crsType": "custom",
      "_mapflow:dataBounds": [1000, 2000, 1500, 2500]
    }
  }
}
```

`_mapflow:*` 前缀是 Mapbox Style Spec 允许的自定义扩展约定。转换层检测到这些字段时创建自定义 Projection + TileGrid。

外部 MapLibre 消费时忽略未知字段，不会报错。

## 编辑器 UX 设计

### 核心原则：ArcGIS 的 UX，Mapbox Style JSON 的内核

制图人员熟悉 ArcGIS 的意图导向配图（"按人口分级着色"），不熟悉 Mapbox 的结构导向配图（写 `fill-color` 表达式）。编辑器向导模式在 UX 层屏蔽 Style JSON 复杂性，同时保留原始表达式编辑入口给技术用户。

### 符号化向导

| ArcGIS 概念 | 编辑器 UX | 底层 Style JSON |
|---|---|---|
| 单一颜色 | 颜色选择器 | `"fill-color": "#ff0000"` |
| 唯一值分类 | 选择字段 + 自动配色 + 微调 | `"fill-color": ["match", ["get","type"], ...]` |
| 分级色彩 | 选择字段 + 色带 + 分级方法 | `"fill-color": ["interpolate", ["linear"], ["get","pop"], ...]` |
| 分级符号 | 选择字段 + 大小范围 | `"circle-radius": ["interpolate", ...]` |

### 标注向导

用户配置：标注字段、字体、大小、颜色、描边、放置方式。
底层自动创建 symbol layer + 配置所有 `text-*` 属性。

### 图层概念

UX 层面一个图层 = 一个数据源 + 一种符号化方式。用户想同时加填充和标注时，在同一图层面板勾选"显示标注"，编辑器底层自动创建 fill layer + symbol layer。

### 高级模式

可折叠面板暴露原始 Style JSON 表达式，与向导双向同步。

### 编辑器布局

```
┌──────────────────────────────────────────────────────────────┐
│ 顶部栏: 地图名称 | 保存 | 发布 | 返回列表                       │
├───────────┬──────────────────────────┬───────────────────────┤
│ 左: 图层管理 │  中央: OL 实时预览         │  右: 符号化向导        │
│           │                          │                       │
│ [+添加图层] │      (OpenLayers)        │ 符号类型选择            │
│           │                          │ 颜色方案               │
│ ☑ 建筑物   │                          │  ○ 单一颜色            │
│   面填充   │                          │  ● 分类               │
│ ☑ 道路    │                          │    字段+色带+预览       │
│   线条    │                          │  ○ 分级色彩            │
│ ☐ 兴趣点   │                          │ 描边/透明度            │
│           │                          │ ▼ 高级(表达式编辑)      │
├───────────┴──────────────────────────┴───────────────────────┤
│ 底部: 图层排序(拖拽) | 缩放范围控制                              │
└──────────────────────────────────────────────────────────────┘
```

## 前端模块

### 新增

| 文件 | 路由/位置 | 职责 |
|------|----------|------|
| `MapsPanel.jsx` | 首页地图 Tab | 地图列表 + 新建/删除/发布/编辑入口 |
| `MapEditor.jsx` | `/editor/:id` | 地图编辑器主页面 |
| `styleJsonToOl.js` | — | Style JSON → OL Flat Style 转换 |
| `SymbolWizard.jsx` | — | 符号化向导（单一颜色/分类/分级） |
| `ColorRampPicker.jsx` | — | 预设色带选择器 |
| `ClassificationEditor.jsx` | — | 分类/分级配置（字段/分级方法/断点） |
| `LabelWizard.jsx` | — | 标注向导 |
| `wizardToStyleJson.js` | — | 向导配置 → Style JSON |
| `styleJsonToWizard.js` | — | Style JSON → 向导配置（反向解析） |
| `MapPublicViewer.jsx` | — | 地图级公开查看器 |

### 移除/修改

| 文件 | 变化 | 时机 |
|------|------|------|
| `StylesPanel.jsx` | 删除 | Phase 1 |
| `ResourcesPanel.jsx` | 移除 Styles tab | Phase 4 |
| `App.jsx` DetailSidebar | 移除 Publish tab | Phase 4 |
| `App.jsx` 文件行操作 | 移除发布/取消发布按钮 | Phase 4 |
| `FontsPanel.jsx` | 移除发布/取消发布按钮 | Phase 4 |
| `main.jsx` | 新增 `/editor/:id` 路由 | Phase 1 |

## 后端模块

### 新增

| 文件 | 职责 |
|------|------|
| `map_handlers.rs` | 地图 CRUD + 发布/取消发布 |
| `map_tiles.rs` | 瓦片代理：sourceId → file_id，复用 tiles.rs/mbtiles.rs/postgis.rs |
| `map_fonts.rs` | 字体代理：fontstack → 已上传字体 → PBF |
| `map_sprites.rs` | icon 集合 → sprite sheet (JSON + PNG) |
| `models.rs` 扩展 | `MapItem`, `CreateMapRequest`, `PublishMapRequest` 等 |

### 移除（Phase 4）

- `published_files` 表及相关端点
- `files.is_public`、`files.public_slug` 字段
- `fonts.is_public`、`fonts.slug` 字段
- `/tiles/{slug}/*` 公开端点
- `/fonts/{ws}/{fontstack}/{range}` 公开端点

## 实施阶段

### Phase 1 — 地图 CRUD + 编辑器基础

- `maps` 表 + CRUD API
- MapsPanel + MapEditor 骨架
- 转换层：fill/line/circle 基础静态属性
- 符号化向导：单一颜色
- 添加数据源为图层，保存 style JSON
- **不动现有发布功能**

### Phase 2 — 编辑器完善 + 表达式

- 转换层：data-driven expressions（`['get',...]`/`['case',...]`/`['interpolate',...]`）
- 符号化向导：唯一值分类、分级色彩
- 标注向导
- 色带选择器
- 字体/图标选择器
- filter 编辑
- 图层排序/显隐
- 高级表达式编辑器

### Phase 3 — 地图发布

- 公开端点：`/maps/{slug}/style.json`、`/maps/{slug}/tiles/*`、`/maps/{slug}/fonts/*`
- sprite 生成
- 嵌入查看器 (`/maps/{slug}/embed`)
- 文档页 (`/maps/{slug}/docs`)
- PMTiles 适配
- **新旧发布并存期**

### Phase 4 — 清理

- 移除数据面板发布入口（DetailSidebar Publish tab、文件行发布按钮）
- 移除字体面板发布入口
- 移除后端旧端点
- 清理 `published_files` 表、`files.is_public`/`files.public_slug`、`fonts.is_public`/`fonts.slug`
- 首页 Tab 调整：`[数据] [资源] [地图]`
- ResourcesPanel 移除 Styles tab

## 风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| opacity 合并到颜色 alpha 时，颜色本身是表达式则合并逻辑复杂 | Phase 2 | Phase 1 仅支持静态 opacity |
| sprite 生成（多图标合成 PNG+JSON） | Phase 3 | 用 `spreet` crate 或 `spritezero` |
| PMTiles 在地图端点下的 Range 代理适配 | Phase 3 | `/maps/{slug}/tiles/{sourceId}` 需支持 HEAD + Range |
| zoom-based 样式 `['zoom']` 映射到 OL `['resolution']` | Phase 2 | 双向转换函数 |
| 编辑器频繁更新 style JSON → 转换 → OL 重建的性能 | Phase 2 | 增量更新 + debounce |
