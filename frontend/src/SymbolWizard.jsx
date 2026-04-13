import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';

const LAYER_TYPES = ['fill', 'line', 'circle'];

const DEFAULT_PAINT = {
  fill: { 'fill-color': '#0080ff', 'fill-opacity': 0.6 },
  line: { 'line-color': '#0080ff', 'line-width': 2 },
  circle: {
    'circle-radius': 6,
    'circle-color': '#ff0040',
    'circle-stroke-color': '#ffffff',
    'circle-stroke-width': 1,
  },
};

export default function SymbolWizard({ paint, layerType, onPaintChange }) {
  const { t } = useTranslation();

  const handleColorChange = useCallback(
    (prop, value) => {
      onPaintChange({ ...paint, [prop]: value });
    },
    [paint, onPaintChange],
  );

  if (layerType === 'fill') {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
        <label style={{ fontSize: '12px', color: '#666' }}>
          {t('map.fillColor')}
          <input
            type="color"
            value={paint['fill-color'] || DEFAULT_PAINT.fill['fill-color']}
            onChange={(e) => handleColorChange('fill-color', e.target.value)}
            style={{ marginLeft: '8px', verticalAlign: 'middle' }}
          />
        </label>
      </div>
    );
  }

  if (layerType === 'line') {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
        <label style={{ fontSize: '12px', color: '#666' }}>
          {t('map.lineColor')}
          <input
            type="color"
            value={paint['line-color'] || DEFAULT_PAINT.line['line-color']}
            onChange={(e) => handleColorChange('line-color', e.target.value)}
            style={{ marginLeft: '8px', verticalAlign: 'middle' }}
          />
        </label>
        <label style={{ fontSize: '12px', color: '#666' }}>
          {t('map.lineWidth')}
          <input
            type="number"
            min="0.5"
            max="20"
            step="0.5"
            value={paint['line-width'] ?? DEFAULT_PAINT.line['line-width']}
            onChange={(e) => handleColorChange('line-width', parseFloat(e.target.value) || 1)}
            style={{ marginLeft: '8px', width: '60px', fontSize: '12px' }}
          />
        </label>
      </div>
    );
  }

  if (layerType === 'circle') {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
        <label style={{ fontSize: '12px', color: '#666' }}>
          {t('map.circleColor')}
          <input
            type="color"
            value={paint['circle-color'] || DEFAULT_PAINT.circle['circle-color']}
            onChange={(e) => handleColorChange('circle-color', e.target.value)}
            style={{ marginLeft: '8px', verticalAlign: 'middle' }}
          />
        </label>
        <label style={{ fontSize: '12px', color: '#666' }}>
          {t('map.circleRadius')}
          <input
            type="number"
            min="1"
            max="50"
            value={paint['circle-radius'] ?? DEFAULT_PAINT.circle['circle-radius']}
            onChange={(e) => handleColorChange('circle-radius', parseInt(e.target.value, 10) || 6)}
            style={{ marginLeft: '8px', width: '60px', fontSize: '12px' }}
          />
        </label>
      </div>
    );
  }

  return null;
}

export { DEFAULT_PAINT, LAYER_TYPES };
