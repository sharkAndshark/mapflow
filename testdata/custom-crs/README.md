# 自定义坐标系测试数据

本目录包含用于测试自定义CRS（无EPSG code）功能的测试数据。

## 文件说明

### simple_custom_crs.geojson
简单测试文件，包含4个features：
- 2个Polygon（Building A, Building B）
- 1个LineString（Road Segment）
- 1个Point（Landmark）

坐标范围：[1000, 2000] 到 [1500, 2500]
CRS: LOCAL_GRID（无EPSG code）

### complex_custom_crs.geojson
复杂测试文件，包含5个features：
- 1个MultiPolygon（building）
- 1个Polygon（park）
- 1个MultiLineString（road）
- 2个Point（utility）

坐标范围：[4800, 2800] 到 [5800, 3900]
CRS: LOCAL_GRID_PROJECT（无EPSG code）

## 预期行为

上传这些文件后：
1. crs_type 应该被标记为 'custom'
2. crs_wkt 应该存储完整的WKT定义
3. tiling_scheme 应该自动计算
4. Preview API 返回的 bbox 应该是原坐标系
5. 瓦片生成应该在原坐标系内进行（不转换到EPSG:3857）

## 使用示例

```bash
# 上传测试文件
curl -X POST -F "file=@simple_custom_crs.geojson" http://localhost:3000/api/uploads

# 等待处理完成，然后获取预览元数据
curl http://localhost:3000/api/files/{id}/preview

# 应该看到：
# {
#   "crs_type": "custom",
#   "crs_wkt": "...",
#   "tiling_scheme": { ... },
#   "bbox": [1000.0, 2000.0, 1500.0, 2500.0]
# }
```
