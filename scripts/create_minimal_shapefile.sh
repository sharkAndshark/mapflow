#!/bin/bash
# Create a minimal test shapefile

echo "Creating test shapefile..."

# Create temporary directory
TEMP_DIR=$(mktemp -d)
SHAPEFILE_NAME="test_points"

# Create .shp file (Shapefile header + data)
python3 << 'EOF'
import struct
import os

def create_minimal_shp():
    shp_content = struct.pack('<i', 9994)  # File code
    shp_content += struct.pack('<i', 0)      # Unused
    shp_content += struct.pack('<i', 0)      # Unused
    shp_content += struct.pack('<i', 0)      # Unused
    shp_content += struct.pack('<i', 0)      # Unused
    shp_content += struct.pack('<i', 0)      # Unused
    shp_content += struct.pack('<i', 0)      # Unused
    shp_content += struct.pack('<i', 0)      # Unused
    shp_content += struct.pack('<i', 1000)   # File length
    shp_content += struct.pack('<i', 1000)   # Version
    shp_content += struct.pack('<i', 1)      # Shape type (Point)
    shp_content += struct.pack('<d', 0.0)    # X min
    shp_content += struct.pack('<d', 0.0)    # Y min
    shp_content += struct.pack('<d', 0.0)    # X max
    shp_content += struct.pack('<d', 0.0)    # Y max
    shp_content += struct.pack('<d', 0.0)    # Z min
    shp_content += struct.pack('<d', 0.0)    # Z max
    shp_content += struct.pack('<d', 0.0)    # M min
    shp_content += struct.pack('<d', 0.0)    # M max
    
    return shp_content

def create_shx():
    shx_content = struct.pack('<i', 9994)  # File code
    shx_content += struct.pack('<i', 0)      # Unused
    shx_content += struct.pack('<i', 0)      # Unused
    shx_content += struct.pack('<i', 0)      # Unused
    shx_content += struct.pack('<i', 0)      # Unused
    shx_content += struct.pack('<i', 0)      # Unused
    shx_content += struct.pack('<i', 0)      # Unused
    shx_content += struct.pack('<i', 0)      # Unused
    shx_content += struct.pack('<i', 50)     # File length
    shx_content += struct.pack('<i', 1000)   # Version
    shx_content += struct.pack('<i', 1)      # Shape type
    
    return shx_content

def create_dbf():
    dbf_content = struct.pack('<B', 0x03)      # Version
    dbf_content += struct.pack('>I', 17038609)    # Update date
    dbf_content += struct.pack('<I', 0)         # Num records
    dbf_content += struct.pack('<H', 32)        # Header length
    dbf_content += struct.pack('<H', 10)        # Record length
    
    # Field descriptor 1: name
    dbf_content += b'name' + b'\x00' * 11
    dbf_content += b'C' + b'\x00'
    dbf_content += struct.pack('<I', 20)         # Field length
    dbf_content += struct.pack('<B', 0)
    dbf_content += struct.pack('<B', 0)
    dbf_content += b'\x00' * 14
    
    # Field descriptor terminator
    dbf_content += b'\r'
    
    return dbf_content

def create_prj():
    prj_content = b'GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563,AUTHORITY["EPSG","7030"]],AUTHORITY["EPSG","6326"]],PRIMEM["Greenwich",0,AUTHORITY["EPSG","8901"]],UNIT["degree",0.0174532925199433,AUTHORITY["EPSG","9122"]],AUTHORITY["EPSG","4326"]]'
    return prj_content

shp_data = create_minimal_shp()
shx_data = create_shx()
dbf_data = create_dbf()
prj_data = create_prj()

TEMP_DIR = os.environ.get('TEMP_DIR', '/tmp/test_shp')
os.makedirs(TEMP_DIR, exist_ok=True)

with open(f'{TEMP_DIR}/test_points.shp', 'wb') as f:
    f.write(shp_data)
with open(f'{TEMP_DIR}/test_points.shx', 'wb') as f:
    f.write(shx_data)
with open(f'{TEMP_DIR}/test_points.dbf', 'wb') as f:
    f.write(dbf_data)
with open(f'{TEMP_DIR}/test_points.prj', 'wb') as f:
    f.write(prj_data)

print(f"Created shapefile in {TEMP_DIR}")
EOF

TEMP_DIR=$(mktemp -d)
export TEMP_DIR

python3 -c "
import os
import struct

shp_content = struct.pack('<i', 9994)  # File code
shp_content += struct.pack('<i', 0) * 7
shp_content += struct.pack('<i', 1000)   # File length
shp_content += struct.pack('<i', 1000)   # Version
shp_content += struct.pack('<i', 1)      # Shape type (Point)
shp_content += struct.pack('<d', 0.0) * 8

dbf_content = struct.pack('<B', 0x03)
dbf_content += struct.pack('>I', 17038609)
dbf_content += struct.pack('<I', 0)
dbf_content += struct.pack('<H', 32)
dbf_content += struct.pack('<H', 10)
dbf_content += b'name' + b'\x00' * 11 + b'C' + b'\x00' + struct.pack('<I', 20) + struct.pack('<B', 0) * 2 + b'\x00' * 14 + b'\r'

prj_content = b'GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",SPHEROID[\"WGS 84\",6378137,298.257223563,AUTHORITY[\"EPSG\",\"7030\"]],AUTHORITY[\"EPSG\",\"6326\"]],PRIMEM[\"Greenwich\",0,AUTHORITY[\"EPSG\",\"8901\"]],UNIT[\"degree\",0.0174532925199433,AUTHORITY[\"EPSG\",\"9122\"]],AUTHORITY[\"EPSG\",\"4326\"]]'

import zipfile
import sys

zip_path = 'data/test_points.zip'
with zipfile.ZipFile(zip_path, 'w') as zf:
    zf.writestr('test_points.shp', shp_content)
    zf.writestr('test_points.shx', shp_content)
    zf.writestr('test_points.dbf', dbf_content)
    zf.writestr('test_points.prj', prj_content)

print(f'Created {zip_path}')
"

echo "Done!"
