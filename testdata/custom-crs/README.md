# 自定义坐标系测试数据

本目录包含用于测试自定义 CRS（无 EPSG code）功能的测试数据。

数据来源：从 OSM 旧金山数据派生（`osm_medium/`）。

## 测试文件

| 文件 | 格式 | 几何类型 | CRS 场景 | 预期行为 |
|------|------|----------|----------|----------|
| `sf_buildings_no_crs.geojson` | GeoJSON | Polygon | 无 CRS 声明 | `crs=null, crs_type=custom` |
| `sf_buildings_custom_wkt.zip` | Shapefile | Polygon | WKT 无 EPSG AUTHORITY | `crs=null, crs_type=custom` |
| `negative_coords_test.geojson` | GeoJSON | Point, Polygon | 无 CRS + 负坐标 | `crs=null, crs_type=custom`, 测试负坐标边界 |

## 数据说明

- **GeoJSON 文件**：每个包含 ~50 个真实 OSM feature
- **Shapefile**：包含自定义投影 WKT（Transverse Mercator，无 EPSG AUTHORITY）
- **negative_coords_test**：边界测试用例，验证负坐标 bbox 计算

## CRS 分类规则

| 输入 CRS | 归一化结果 | crs_type |
|----------|------------|----------|
| NULL（无声明） | NULL | custom |
| EPSG:XXXX | EPSG:XXXX | standard |
| WGS84 / CRS84 | EPSG:4326 | standard |
| WKT + AUTHORITY["EPSG"] | EPSG:XXXX | standard |
| WKT 无 EPSG AUTHORITY | NULL (GDAL 不返回 WKT) | custom |
| 其他字符串 | 原值 | custom |
