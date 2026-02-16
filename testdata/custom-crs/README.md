# 自定义坐标系测试数据

本目录包含用于测试自定义 CRS（无 EPSG code）功能的测试数据。

## 测试文件

| 文件 | CRS 场景 | 坐标范围 | 几何类型 | 预期行为 |
|------|----------|----------|----------|----------|
| `simple_custom_crs.geojson` | 无 CRS 声明 | [1000, 2000] - [1500, 2500] | Point, LineString, Polygon | crs=null, crs_type=custom |
| `complex_custom_crs.geojson` | 自定义 CRS 名称 | [4800, 2800] - [5800, 3900] | Point, MultiLineString, Polygon, MultiPolygon | crs="LOCAL_GRID_PROJECT", crs_type=custom |
| `no_crs_test.geojson` | 无 CRS 声明 | [950, 2000] - [1400, 2400] | Point, LineString, Polygon | crs=null, crs_type=custom |
| `negative_coords_test.geojson` | 无 CRS + 负坐标 | [-500, -300] - [-250, -150] | Point, Polygon | crs=null, crs_type=custom, 测试负坐标处理 |

## 预期行为

上传这些文件后：

1. `crs_type` 应该被标记为 `'custom'`
2. `data_bounds` 应该存储原始坐标范围（JSON 格式）
3. Preview API 返回的 `bbox` 应该直接使用 `data_bounds`（不做坐标转换）
4. 瓦片生成应该在原坐标系内进行（不转换到 EPSG:3857）
5. 前端应该使用自定义 TileGrid 和 Projection

## API 响应示例

### Preview API 响应

```json
{
  "id": "abc123",
  "name": "no_crs_test",
  "crs": null,
  "crsType": "custom",
  "bbox": [950.0, 2000.0, 1400.0, 2400.0],
  "dataBounds": [950.0, 2000.0, 1400.0, 2400.0],
  "tileFormat": null,
  "minZoom": null,
  "maxZoom": null
}
```

### File List API 响应

```json
{
  "id": "abc123",
  "name": "no_crs_test",
  "crs": null,
  "crsType": "custom",
  ...
}
```

## 使用示例

```bash
# 上传测试文件
curl -X POST -F "file=@no_crs_test.geojson" http://localhost:3000/api/uploads

# 等待处理完成，然后获取预览元数据
curl http://localhost:3000/api/files/{id}/preview

# 请求瓦片
curl http://localhost:3000/api/files/{id}/tiles/0/0/0 --output tile.mvt
```

## CRS 分类规则

| 输入 CRS | 归一化结果 | crs_type |
|----------|------------|----------|
| NULL（无声明） | NULL | custom |
| EPSG:XXXX | EPSG:XXXX | standard |
| WGS84 / CRS84 | EPSG:4326 | standard |
| WKT + AUTHORITY["EPSG"] | EPSG:XXXX | standard |
| WKT 无 EPSG AUTHORITY | 完整 WKT | custom |
| 其他字符串 | 原值 | custom |
