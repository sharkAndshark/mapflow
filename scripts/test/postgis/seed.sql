CREATE EXTENSION IF NOT EXISTS postgis;

DROP VIEW IF EXISTS public.roads_view;
DROP TABLE IF EXISTS public.roads_quoted;
DROP TABLE IF EXISTS public.roads_composite;
DROP TABLE IF EXISTS public.roads;

CREATE TABLE public.roads (
  id BIGINT PRIMARY KEY,
  name TEXT NOT NULL,
  category TEXT,
  speed_limit INTEGER,
  geom geometry(LineString, 4326) NOT NULL
);

INSERT INTO public.roads (id, name, category, speed_limit, geom) VALUES
  (1, 'Main Street', 'primary', 50, ST_GeomFromText('LINESTRING(-122.4231 37.7785, -122.4120 37.7810)', 4326)),
  (2, '2nd Avenue', 'secondary', 35, ST_GeomFromText('LINESTRING(-122.4300 37.7700, -122.4200 37.7680)', 4326));

CREATE INDEX roads_geom_gix ON public.roads USING GIST (geom);

CREATE VIEW public.roads_view AS
SELECT id, name, category, speed_limit, geom
FROM public.roads;

CREATE TABLE public.roads_composite (
  id BIGINT NOT NULL,
  version INTEGER NOT NULL,
  name TEXT NOT NULL,
  geom geometry(LineString, 4326) NOT NULL,
  UNIQUE (id, version)
);

INSERT INTO public.roads_composite (id, version, name, geom) VALUES
  (1, 1, 'Main Street v1', ST_GeomFromText('LINESTRING(-122.4231 37.7785, -122.4120 37.7810)', 4326)),
  (1, 2, 'Main Street v2', ST_GeomFromText('LINESTRING(-122.4231 37.7786, -122.4120 37.7811)', 4326));

CREATE INDEX roads_composite_geom_gix ON public.roads_composite USING GIST (geom);

CREATE TABLE public.roads_quoted (
  id BIGINT PRIMARY KEY,
  "road name" TEXT NOT NULL,
  "1st-class" INTEGER,
  geom geometry(LineString, 4326) NOT NULL
);

INSERT INTO public.roads_quoted (id, "road name", "1st-class", geom) VALUES
  (1, 'Quoted Main', 1, ST_GeomFromText('LINESTRING(-122.4231 37.7785, -122.4120 37.7810)', 4326)),
  (2, 'Quoted Second', 2, ST_GeomFromText('LINESTRING(-122.4300 37.7700, -122.4200 37.7680)', 4326));

CREATE INDEX roads_quoted_geom_gix ON public.roads_quoted USING GIST (geom);
