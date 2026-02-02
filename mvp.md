## MapNodes

Publishing your tiles serverice with simply draging! MapNodes是一个rust语言编写的高性能地图服务器，他支持用户通过简单的拖拽节点，连接节点来发布自己的地图服务。

## 可观测行为

1. 启动后自动监听3000端口
2. 启动后自动在同目录寻找nodes.duckdb文件，若不存在则创建该文件，并load spatial extension
3. 启动后自动在同目录寻找data文件夹，用来保存上传的文件，若不存在则创建
4. 启动后自动在同目录寻找config.json文件，若不存在，不创建。只在控制台提示没有找到config.json
5. 有上传文件接口，允许用户上传shapefile(shapefile zip包），目前只允许shapefile zip类型。
6. 考虑到有shapefile压缩包内没有.prj文件的情况（其他格式也一样可能存在未定义坐标系的情况其实），上传接口应该有可选参数srid。
7. 上传接口应该校验有没有valid的坐标系定义，没有则返回 HTTP 400 错误
8. 上传时会导入到duckdb内的一个table,table名称格式为 文件名_guid
9. 有get config.json的接口
10. 有appLY config.json 的接口，该接口会校验，如果通过校验，则覆盖本地的json文件，并重启当前服务器
11. 有verify config 接口，该接口会校验提供的config.json，帮助前端用户检验自己的托拖拽拽
12. http://3000/ 是系统的首页，会呈现一个非常简单的react编写的页面。
13. 首页会自动调取get config接口，然后渲染成节点连接成的网络结构。
14. 用户可以通过上传shapefile的方式，创建新的resource节点（只读）。
15. 用户可以右键单击，新增新的xyz节点或layer节点。
16. 首页有validate按钮，用户可以validate当前的网络节点
17. 首页有apply按钮，用户可以应用当前网络节点
18. 系统有三种节点类型：resource节点（只读，上传shapefile创建）、layer节点（字段选择、缩放设置）、xyz节点（聚合layers）。通过edges连接节点。

## config.json核心定义

**改造说明：** 架构简化为3种节点类型（resource、layer、xyz），通过edges数组显式连接，移除了独立的resources数组。

```jsonc
{
    "version": "0.1.0",
    "nodes": [
        // Resource节点（只读，上传shapefile自动创建）
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
        // Layer节点（可编辑）
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
        // XYZ节点（可编辑）
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


## 关键变化总结

| 变化项 | 修改前 | 修改后 |
|--------|--------|--------|
| 节点类型 | data_node、auto_xyz_node | resource、layer、xyz |
| resources | 独立数组 | 作为节点类型 |
| duckdb_table_names | 数组支持多表 | 单个table_name |
| 连接方式 | 内嵌引用 | edges数组 |
| 字段选择 | 在data_node | 在layer |
| 缩放设置 | 在xyz的layers中 | 在layer节点 |
| layer定义 | xyz的子属性 | 独立节点 |

## 关键文档链接

1. https://crates.io/crates/duckdb duckdb crate href, 如果duckdb crate构建过慢，参考这里
2. 
