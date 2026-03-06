CREATE EXTENSION IF NOT EXISTS postgis;

DROP VIEW IF EXISTS public.roads_view;
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
