import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getFieldValues } from './api.js';
import { getSchema } from './api.js';
import ColorRampSelector, { resolveRampColors } from './ColorRamp.jsx';

const CLASSIFY_MODES = ['single', 'unique', 'graduated'];

function buildSingleColorPaint(layerType, color) {
  const opacityKey = `${layerType}-opacity`;
  if (layerType === 'fill') return { 'fill-color': color, 'fill-opacity': 0.7 };
  if (layerType === 'line') return { 'line-color': color, 'line-width': 2 };
  if (layerType === 'circle')
    return {
      'circle-color': color,
      'circle-radius': 6,
      'circle-stroke-color': '#ffffff',
      'circle-stroke-width': 1,
    };
  return {};
}

function buildUniqueValuePaint(layerType, fieldName, valueColorMap) {
  const entries = Object.entries(valueColorMap);
  if (entries.length === 0) return {};
  const cases = [];
  for (const [value, color] of entries) {
    cases.push(['==', ['get', fieldName], value]);
    cases.push(color);
  }
  const fallback = entries[0][1];
  if (layerType === 'fill') {
    return { 'fill-color': ['case', ...cases, fallback], 'fill-opacity': 0.7 };
  }
  if (layerType === 'line') {
    return { 'line-color': ['case', ...cases, fallback], 'line-width': 2 };
  }
  if (layerType === 'circle') {
    return { 'circle-color': ['case', ...cases, fallback], 'circle-radius': 6 };
  }
  return {};
}

function buildGraduatedPaint(layerType, fieldName, stops) {
  if (stops.length === 0) return {};
  const cases = [];
  for (let i = 0; i < stops.length; i++) {
    cases.push(['<=', ['get', fieldName], stops[i].value]);
    cases.push(stops[i].color);
  }
  const fallback = stops[stops.length - 1]?.color || '#888888';
  if (layerType === 'fill') {
    return { 'fill-color': ['case', ...cases, fallback], 'fill-opacity': 0.7 };
  }
  if (layerType === 'line') {
    return { 'line-color': ['case', ...cases, fallback], 'line-width': 2 };
  }
  if (layerType === 'circle') {
    return { 'circle-color': ['case', ...cases, fallback], 'circle-radius': 6 };
  }
  return {};
}

export default function ClassificationWizard({ sourceId, layerType, paint, onPaintChange }) {
  const { t } = useTranslation();
  const [mode, setMode] = useState('single');
  const [fields, setFields] = useState([]);
  const [selectedField, setSelectedField] = useState('');
  const [fieldData, setFieldData] = useState(null);
  const [rampName, setRampName] = useState('Set1');
  const [classCount, setClassCount] = useState(5);
  const [singleColor, setSingleColor] = useState('#0080ff');
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    if (!sourceId) return;
    let cancelled = false;
    getSchema(sourceId)
      .then((data) => {
        if (!cancelled && data.layers?.length > 0) {
          const f = data.layers[0].fields || [];
          setFields(
            f.filter(
              (x) =>
                x.type !== 'Geometry' &&
                x.type !== 'Point' &&
                x.type !== 'LineString' &&
                x.type !== 'Polygon',
            ),
          );
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [sourceId]);

  useEffect(() => {
    if (!sourceId || !selectedField || mode === 'single') return;
    let cancelled = false;
    setIsLoading(true);
    getFieldValues(sourceId, selectedField, mode === 'unique' ? 50 : 0)
      .then((data) => {
        if (!cancelled) setFieldData(data);
      })
      .catch(() => {})
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sourceId, selectedField, mode]);

  useEffect(() => {
    if (mode === 'single') {
      const propKey =
        layerType === 'fill' ? 'fill-color' : layerType === 'line' ? 'line-color' : 'circle-color';
      const existing = paint?.[propKey];
      if (typeof existing === 'string') setSingleColor(existing);
      onPaintChange(buildSingleColorPaint(layerType, singleColor));
    }
  }, []);

  const applyClassification = useCallback(() => {
    if (mode === 'single') {
      onPaintChange(buildSingleColorPaint(layerType, singleColor));
    } else if (mode === 'unique' && fieldData) {
      const values = fieldData.values.map((v) => (typeof v === 'string' ? v : String(v)));
      const colors = resolveRampColors(rampName, values.length);
      const map = {};
      values.forEach((v, i) => {
        map[v] = colors[i] || '#888888';
      });
      onPaintChange(buildUniqueValuePaint(layerType, selectedField, map));
    } else if (mode === 'graduated' && fieldData?.min != null && fieldData?.max != null) {
      const { min, max } = fieldData;
      const count = Math.max(2, classCount);
      const step = (max - min) / count;
      const colors = resolveRampColors(rampName, count);
      const stops = [];
      for (let i = 0; i < count; i++) {
        stops.push({ value: min + step * (i + 1), color: colors[i] });
      }
      onPaintChange(buildGraduatedPaint(layerType, selectedField, stops));
    }
  }, [mode, layerType, singleColor, selectedField, fieldData, rampName, classCount, onPaintChange]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
      <div>
        <label style={{ fontSize: '11px', color: '#666', display: 'block', marginBottom: '4px' }}>
          {t('map.classificationMode')}
        </label>
        <select
          value={mode}
          onChange={(e) => setMode(e.target.value)}
          style={{ fontSize: '12px', width: '100%', padding: '4px' }}
        >
          <option value="single">{t('map.singleColor')}</option>
          <option value="unique">{t('map.uniqueValue')}</option>
          <option value="graduated">{t('map.graduatedColor')}</option>
        </select>
      </div>

      {mode === 'single' && (
        <label style={{ fontSize: '12px', color: '#666' }}>
          {t('map.fillColor')}
          <input
            type="color"
            value={singleColor}
            onChange={(e) => {
              setSingleColor(e.target.value);
              onPaintChange(buildSingleColorPaint(layerType, e.target.value));
            }}
            style={{ marginLeft: '8px', verticalAlign: 'middle' }}
          />
        </label>
      )}

      {mode !== 'single' && (
        <>
          <div>
            <label
              style={{ fontSize: '11px', color: '#666', display: 'block', marginBottom: '4px' }}
            >
              {t('map.classifyField')}
            </label>
            <select
              value={selectedField}
              onChange={(e) => setSelectedField(e.target.value)}
              style={{ fontSize: '12px', width: '100%', padding: '4px' }}
            >
              <option value="">{t('map.selectField')}</option>
              {fields.map((f) => (
                <option key={f.name} value={f.name}>
                  {f.alias || f.name} ({f.type})
                </option>
              ))}
            </select>
          </div>

          {selectedField && (
            <div>
              <label
                style={{ fontSize: '11px', color: '#666', display: 'block', marginBottom: '4px' }}
              >
                {t('map.colorRamp')}
              </label>
              <ColorRampSelector
                mode={mode === 'unique' ? 'categorical' : 'sequential'}
                value={rampName}
                onChange={setRampName}
              />
            </div>
          )}

          {mode === 'graduated' && selectedField && (
            <div>
              <label
                style={{ fontSize: '11px', color: '#666', display: 'block', marginBottom: '4px' }}
              >
                {t('map.classCount')}
              </label>
              <input
                type="number"
                min={2}
                max={15}
                value={classCount}
                onChange={(e) => setClassCount(parseInt(e.target.value, 10) || 5)}
                style={{ fontSize: '12px', width: '60px', padding: '4px' }}
              />
              {fieldData?.min != null && (
                <span style={{ fontSize: '11px', color: '#888', marginLeft: '8px' }}>
                  {fieldData.min.toFixed(2)} – {fieldData.max.toFixed(2)}
                </span>
              )}
            </div>
          )}

          {isLoading && (
            <span style={{ fontSize: '11px', color: '#888' }}>{t('common.loading')}</span>
          )}

          {selectedField && !isLoading && (
            <button
              type="button"
              className="btn-primary"
              style={{ fontSize: '12px', padding: '4px 12px' }}
              onClick={applyClassification}
            >
              {t('map.apply')}
            </button>
          )}
        </>
      )}
    </div>
  );
}
