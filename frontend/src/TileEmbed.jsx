import React from 'react';
import { useParams } from 'react-router-dom';

import { PublicTileMap, usePublicTileMeta } from './PublicTileViewer.jsx';

export default function TileEmbed() {
  const { slug } = useParams();
  const { meta, error, isLoading } = usePublicTileMeta(slug);

  return (
    <main
      data-testid="tile-embed-page"
      style={{ width: '100%', minHeight: '100dvh', height: '100dvh', background: '#f5f4f2' }}
    >
      <PublicTileMap
        meta={meta}
        error={error}
        isLoading={isLoading}
        overlayLabel={null}
        dataTestId="tile-embed-map"
        exposeMapGlobally
      />
    </main>
  );
}
