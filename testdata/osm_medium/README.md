# OSM Medium Test Data

Source: OpenStreetMap (Overpass API)
Date: 2026-02-04
BBox: -122.45,37.76,-122.38,37.80 (San Francisco)
CRS: EPSG:4326

Contents:
- geojson/
  - sf_points.geojson (points: traffic signals, places, barriers, man_made, amenity tags)
  - sf_lines.geojson (lines: highway/railway/waterway)
  - sf_polygons.geojson (multipolygons: building/landuse/natural/amenity)
- shapefile/
  - sf_points.zip (shapefile zip)
  - sf_lines.zip (shapefile zip)
  - sf_polygons.zip (shapefile zip, fields limited to osm_id,name,building,amenity)

Notes:
- Shapefile DBF fields may truncate long tag strings and may not preserve non-ASCII perfectly.
- These files are intended for upload/listing tests and manual validation.
