#!/bin/bash

# MapFlow Test Data Preparation Script
# Downloads OSM data and converts to shapefile format

set -e

echo "Preparing test data for MapFlow..."

# Check if ogr2ogr is installed
if ! command -v ogr2ogr &> /dev/null; then
    echo "Error: ogr2ogr is not installed"
    echo "Please install GDAL:"
    echo "  brew install gdal"
    exit 1
fi

# Create data directory
mkdir -p data/test_data

# Download OSM data for a small area (Shanghai center)
echo "Downloading OSM data for Shanghai center..."
curl -o data/test_data/shanghai.osm.pbf \
  "https://download.geofabrik.de/asia/china/shanghai-latest.osm.pbf"

# Convert OSM to GeoPackage first (more reliable)
echo "Converting OSM to GeoPackage..."
ogr2ogr -f "GPKG" \
  data/test_data/shanghai.gpkg \
  data/test_data/shanghai.osm.pbf \
  points lines multipolygons

# Extract buildings (polygons with building tags)
echo "Extracting buildings..."
ogr2ogr -f "ESRI Shapefile" \
  -where "building IS NOT NULL" \
  data/test_data/shanghai_buildings \
  data/test_data/shanghai.gpkg \
  multipolygons

# Extract coffee shops (points with amenity=coffee_shop or cafe)
echo "Extracting coffee shops..."
ogr2ogr -f "ESRI Shapefile" \
  -where "amenity = 'coffee_shop' OR amenity = 'cafe'" \
  data/test_data/shanghai_coffees \
  data/test_data/shanghai.gpkg \
  points

# Extract roads (lines with highway tag)
echo "Extracting roads..."
ogr2ogr -f "ESRI Shapefile" \
  -where "highway IS NOT NULL" \
  data/test_data/shanghai_roads \
  data/test_data/shanghai.gpkg \
  lines

# Create ZIP archives for each shapefile
echo "Creating ZIP archives..."

# Buildings
cd data/test_data
zip -r shanghai_buildings.zip shanghai_buildings.*
cd ../..

# Coffee shops
cd data/test_data
zip -r shanghai_coffees.zip shanghai_coffees.*
cd ../..

# Roads
cd data/test_data
zip -r shanghai_roads.zip shanghai_roads.*
cd ../..

echo "Test data preparation complete!"
echo ""
echo "Created files:"
echo "  - data/test_data/shanghai_buildings.zip"
echo "  - data/test_data/shanghai_coffees.zip"
echo "  - data/test_data/shanghai_roads.zip"
echo ""
echo "You can now use these files for testing the upload functionality."
