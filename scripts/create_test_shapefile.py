#!/usr/bin/env python3
"""
Create a simple test shapefile for MapFlow testing
"""
import os
import tempfile
import zipfile

def create_minimal_shapefile():
    """Create a minimal valid shapefile"""
    from osgeo import ogr, osr

    # Create spatial reference (WGS84)
    srs = osr.SpatialReference()
    srs.ImportFromEPSG(4326)

    # Create driver
    driver = ogr.GetDriverByName("ESRI Shapefile")
    
    temp_dir = tempfile.mkdtemp()
    shapefile_name = "test_points"
    shapefile_path = os.path.join(temp_dir, shapefile_name)
    
    # Create datasource
    ds = driver.CreateDataSource(shapefile_path)
    
    # Create layer
    layer = ds.CreateLayer("test_points", srs, ogr.wkbPoint)
    
    # Add fields
    layer.CreateField(ogr.FieldDefn("name", ogr.OFTString))
    layer.CreateField(ogr.FieldDefn("value", ogr.OFTReal))
    
    # Create features
    points = [
        (-74.006, 40.7128, "New York", 100),
        (-0.1276, 51.5074, "London", 200),
        (139.6917, 35.6895, "Tokyo", 300),
    ]
    
    for lon, lat, name, value in points:
        feature = ogr.Feature(layer.GetLayerDefn())
        point = ogr.Geometry(ogr.wkbPoint)
        point.AddPoint(lon, lat)
        feature.SetGeometry(point)
        feature.SetField("name", name)
        feature.SetField("value", value)
        layer.CreateFeature(feature)
    
    # Cleanup
    ds = None
    
    # Create ZIP
    zip_path = os.path.join("data", "test_points.zip")
    os.makedirs("data", exist_ok=True)
    
    with zipfile.ZipFile(zip_path, 'w') as zipf:
        for ext in ['.shp', '.shx', '.dbf', '.prj', '.cpg']:
            file_path = f"{shapefile_path}{ext}"
            if os.path.exists(file_path):
                zipf.write(file_path, f"{shapefile_name}{ext}")
    
    print(f"Created test shapefile: {zip_path}")
    return zip_path

if __name__ == "__main__":
    try:
        create_minimal_shapefile()
    except ImportError:
        print("GDAL Python bindings not available, skipping shapefile creation")
        print("You can upload your own shapefile for testing")
    except Exception as e:
        print(f"Failed to create test shapefile: {e}")
