## MapNodes

Publishing your tiles serverice with simply draging! MapNodes是一个rust语言编写的高性能地图服务器，他支持用户通过简单的拖拽节点，连接节点来发布自己的地图服务。

## 可观测行为

1. 启动后自动监听3000端口
2. 启动后自动在同目录寻找nodes.duckdb文件，若不存在则创建该文件，并load spatial extension
3. 启动后自动在同目录寻找data文件夹，用来保存上传的文件，若不存在则创建
4. 启动后自动在同目录寻找tmp文件夹，用来做一些临时操作，若不存在则创建。反思，这个文件夹有存在的必要吗？
5. 启动后自动在同目录寻找config.json文件，若不存在，不创建。只在控制台提示没有找到config.json
6. 有上传文件接口，允许用户上传shapefile(shapefile zip包），目前只允许shapefile zip类型。
7. 考虑到有shapefile压缩包内没有.prj文件的情况（其他格式也一样可能存在未定义坐标系的情况其实），上传接口应该有可选参数srid。
8. 上传接口应该校验有没有valid的坐标系定义，没有则返回400？
9. 上传时会导入到duckdb内的一个table,table名称格式为 文件名_guid
10. 有get config.json的接口
11. 有appLY config.json 的接口，该接口会校验，如果通过校验，则覆盖本地的json文件，并重启当前服务器
12. 有verify config 接口，该接口会校验提供的config.json，帮助前端用户检验自己的托拖拽拽
13. http://3000/ 是系统的首页，会呈现一个非常简单的react编写的页面。
14. 首页会自动调取get config接口，然后渲染成节点连接成的网络结构。
15. 用户可以通过上传shapefile的方式，创建新的shapefile节点。
16. 用户可以右键单击，新增新的xyz节点。
17. 首页有validate按钮，用户可以validate当前的网络节点
18. 首页有apply按钮，用户可以应用当前网络节点
19. 用户只允许创建data节点以及xyz节点，我们目前也只有这两种节点。文件是一种资源，不是节点。

## config.json核心定义

```jsonc
{
    "version": "0.0.1",
    "resources": [
        {
            "id": "3E8F1D0220E9B0EE50D4347A46D18CBD", // required,程序生成，禁止人类修改
            "type": "shapefile", // required,程序生成，禁止人类修改，该类型node只能新增，删除，禁止更新，具有不变性
            "name": "shanghai_buildings", // optional,允许人类更改，方便人类阅读记忆，允许重复
            "description": "shanghai_buildings", //optional,允许人类更改，方便人类阅读记忆，允许重复
            "file_path": ["/path/to/shanghai_buildings.shp"],// required,考虑到shapefile这种文件可能有多个文件，我们允许file_path是数组。未来增加geotiff也一样可能有这种情况，比如overlay文件. 由程序生成，禁止人类修改
            "size": 9999, // required,单位是字节，程序生成，禁止人类修改
            "create_timestamp": 1769874490, // required, 程序生成，禁止人类修改
            "hash": "", // required,哪种计算HASH的方式最快，我们应考虑最快的方法，不要担心碰撞，概率太小了？？xxHash? 程序生成，禁止人类修改
            "srid": "4326",//  required, 程序生成，禁止人类修改,考虑到有可能出现用户自己定义的坐标系，eg, epsg没有对应的code,因此采用了字符串格式，未来我们会支持自定义坐标系
            "duckdb_table_names": [ // required, 数组，支持一个资源对应多张表（如 geojson 导出多表）
                "shanghai_EB24274F4992232980E515AD2F977EFA"
            ]
        },
        {
            "id": "7297D7AEB16ADE5D9B6C478128BA3D27", // required,程序生成，禁止人类修改
            "type": "shapefile", // required,程序生成，禁止人类修改，该类型node只能新增，删除，禁止更新，具有不变性
            "name": "shanghai_coffees", // optional,允许人类更改，方便人类阅读记忆，允许重复
            "description": "shanghai coffees", //optional,允许人类更改，方便人类阅读记忆，允许重复
            "file_path": ["/path/to/shanghai_coffees.shp"],// required,考虑到shapefile这种文件可能有多个文件，我们允许file_path是数组。未来增加geotiff也一样可能有这种情况，比如overlay文件. 由程序生成，禁止人类修改
            "size": 8888, // required,单位是字节，程序生成，禁止人类修改
            "create_timestamp": 1769871490, // required, 程序生成，禁止人类修改
            "hash": "", // required,哪种计算HASH的方式最快，我们应考虑最快的方法，不要担心碰撞，概率太小了？？xxHash? 程序生成，禁止人类修改
            "srid": "4326",//  required, 程序生成，禁止人类修改,考虑到有可能出现用户自己定义的坐标系，eg, epsg没有对应的code,因此采用了字符串格式。
            "duckdb_table_names": [ // required, 数组，支持一个资源对应多张表
                "shanghai_2025_EB24274F4992232980E515AD2F977EFA"
            ]
    },
    ],
    "nodes": [
        {
            "id": "6A5FA5034DD93B89865F410C118FD4CC",
            "type": "data_node",
            "source_resource_id": "7297D7AEB16ADE5D9B6C478128BA3D27", // required, 引用的 resource id
            "duckdb_table_name": "shanghai_2025_EB24274F4992232980E515AD2F977EFA", // required, 从 resource 的 duckdb_table_names 中选择的具体表名
            "name": "shanghai_coffees", // optional, 默认继承 resource 的 name
            "description": "shanghai coffees", // optional, 默认继承 resource 的 description
            "srid": "4326", // required, 继承自 resource，只读
            "fields": ["field1", "field2", "field3"] // required, 字段选择，默认为全部字段
        },        {
            "id": "37DF92130D77B9770CEAD17A9008308F",
            "type": "data_node",
            "source_resource_id": "3E8F1D0220E9B0EE50D4347A46D18CBD", // required, 引用的 resource id
            "duckdb_table_name": "shanghai_EB24274F4992232980E515AD2F977EFA", // required, 从 resource 的 duckdb_table_names 中选择的具体表名
            "name": "shanghai_buildings", // optional, 默认继承 resource 的 name
            "description": "shanghai buildings", // optional, 默认继承 resource 的 description
            "srid": "4326", // required, 继承自 resource，只读
            "fields": ["field2", "field3"] // required, 字段选择，默认为全部字段
        },
    {
        "id": "6304FB8CBF7D84B547E39582B5BDD422", // required 程序生成，禁止人类修改
        "type":"auto_xyz_node", // required 程序生成，禁止人类修改 
        "name": "shanghai_pois", // optional,允许人类更改，方便人类阅读记忆，允许重复
        "description": "shanghai POI", //optional,允许人类更改，方便人类阅读记忆，允许重复
        "center": [-76.275329586789, 39.153492567373, 8], // OPTIONAL. Array. Default: null. The first value is the longitude, the second is latitude . the third value is the zoom level as an integer. Longitude and latitude MUST be within the specified bounds. The zoom level MUST be between minzoom and maxzoom  允许人类更改，但zoom必须满足要求
        "min_zoom": 8, // optional, 程序生成，人类可改,默认8
        "max_zoom": 10, // optional, 程序生成，人类可改,默认24
        "schema" : "xyz", // optional, 程序生成，人类不可改。固定为xyz
        "fillzoom": 6, // optional, 程序生成，人类可改，默认为8
        "srid": "4326", // required, 
        "bounds" : [ -180, -85.05112877980659, 180, 85.0511287798066 ] , // OPTIONAL, 程序生成，人类可改。
        "layers": [
            {
                "id": "layer_buildings", // required, 图层唯一标识
                "source_node_id": "37DF92130D77B9770CEAD17A9008308F", // required, 引用的 data_node id
                "minzoom": 8, // optional, 该图层的最小缩放级别
                "maxzoom": 14 // optional, 该图层的最大缩放级别
            },
            {
                "id": "layer_coffees", // required, 图层唯一标识
                "source_node_id": "6A5FA5034DD93B89865F410C118FD4CC", // required, 引用的 data_node id
                "minzoom": 10, // optional, 该图层的最小缩放级别
                "maxzoom": 18 // optional, 该图层的最大缩放级别
            }
        ]
    }
]
}
```


## 关键文档链接

1. https://crates.io/crates/duckdb duckdb crate href, 如果duckdb crate构建过慢，参考这里
2. 
