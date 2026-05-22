import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getFieldValues, getSchema } from './api.js';
import ColorRampSelector, { resolveRampColors } from './ColorRamp.jsx';
import { equalIntervalBreaks, quantileBreaks, jenksBreaks } from './classification.js';
import { filterToExpr } from './styleJsonToOl.js';

const RENDERER_TYPES = [
  { value: 'single', icon: '■' },
  { value: 'categorized', icon: '▦' },
  { value: 'graduated', icon: '▤' },
  { value: 'proportional', icon: '◉' },
  { value: 'rules', icon: '⊞' },
  { value: 'none', icon: '∅' },
];

const LINE_STYLES = [
  { value: 'solid', label: '───' },
  { value: 'dashed', label: '- - -' },
  { value: 'dotted', label: '· · ·' },
  { value: 'dashdot', label: '-·-·' },
];

const LINE_CAPS = [
  { value: 'butt', label: 'Butt' },
  { value: 'round', label: 'Round' },
  { value: 'square', label: 'Square' },
];

const LINE_JOINS = [
  { value: 'mitre', label: 'Miter' },
  { value: 'bevel', label: 'Bevel' },
  { value: 'round', label: 'Round' },
];

function CollapsibleSection({ title, defaultOpen = true, children }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div style={{ borderBottom: '1px solid #eee' }}>
      <button
        type="button"
        onClick={() => setOpen(!open)}
        style={{
          width: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '8px 0',
          background: 'none',
          border: 'none',
          cursor: 'pointer',
          fontSize: '12px',
          fontWeight: 600,
          color: '#444',
        }}
      >
        <span>{title}</span>
        <span style={{ fontSize: '10px', color: '#aaa' }}>{open ? '▼' : '▶'}</span>
      </button>
      {open && <div style={{ paddingBottom: '10px' }}>{children}</div>}
    </div>
  );
}

function summarizeRule(rule) {
  if (!rule.filter?.conditions?.length) return null;
  return rule.filter.conditions
    .filter((c) => c.field && c.value !== '')
    .map((c) => `${c.field} ${c.operator} ${c.value}`)
    .join(' & ');
}

function RuleConditionEditor({ fields, condition, onChange, onRemove }) {
  return (
    <div style={{ display: 'flex', gap: '4px', alignItems: 'center', flexWrap: 'wrap' }}>
      <select
        value={condition.field}
        onChange={(e) => onChange({ ...condition, field: e.target.value })}
        style={{ fontSize: '11px', flex: '1 1 70px', minWidth: '60px', padding: '2px' }}
      >
        <option value="">--</option>
        {fields.map((f) => (
          <option key={f.name} value={f.name}>
            {f.alias || f.name}
          </option>
        ))}
      </select>
      <select
        value={condition.operator}
        onChange={(e) => onChange({ ...condition, operator: e.target.value })}
        style={{ fontSize: '11px', width: '60px', padding: '2px' }}
      >
        {FILTER_OPERATORS.map((op) => (
          <option key={op.value} value={op.value}>
            {op.label}
          </option>
        ))}
      </select>
      <input
        type="text"
        value={condition.value ?? ''}
        placeholder="value"
        onChange={(e) => onChange({ ...condition, value: e.target.value })}
        style={{ fontSize: '11px', flex: '1 1 50px', minWidth: '40px', padding: '2px 4px' }}
      />
      <button
        type="button"
        onClick={onRemove}
        style={{
          background: 'none',
          border: 'none',
          color: '#c00',
          cursor: 'pointer',
          fontSize: '12px',
          padding: '0 2px',
          lineHeight: 1,
        }}
      >
        ×
      </button>
    </div>
  );
}

function RuleEditor({
  rule,
  fields,
  layerType,
  onChange,
  onDelete,
  onMoveUp,
  onMoveDown,
  isFirst,
  isLast,
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const enabled = rule.enabled !== false;
  const summary = summarizeRule(rule);

  function updateConditions(conditions) {
    onChange({ ...rule, filter: { conditions } });
  }

  return (
    <div
      style={{
        border: '1px solid #e0e0e0',
        borderRadius: '4px',
        padding: '6px',
        marginBottom: '4px',
        background: enabled ? '#fff' : '#f9f9f9',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
        <input
          type="checkbox"
          checked={enabled}
          onChange={(e) => onChange({ ...rule, enabled: e.target.checked })}
          style={{ cursor: 'pointer' }}
        />
        <input
          type="color"
          value={rule.color || '#888888'}
          onChange={(e) => onChange({ ...rule, color: e.target.value })}
          style={{
            width: '24px',
            height: '18px',
            border: '1px solid #ddd',
            borderRadius: '2px',
            cursor: 'pointer',
          }}
        />
        <span
          style={{
            flex: 1,
            fontSize: '11px',
            color: '#444',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {summary || t('map.ruleElse')}
        </span>
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          style={{
            background: 'none',
            border: 'none',
            cursor: 'pointer',
            fontSize: '11px',
            color: '#666',
            padding: '0 2px',
          }}
        >
          {expanded ? '△' : '▽'}
        </button>
        <button
          type="button"
          onClick={onMoveUp}
          disabled={isFirst}
          style={{
            background: 'none',
            border: 'none',
            cursor: isFirst ? 'default' : 'pointer',
            fontSize: '10px',
            padding: '0 2px',
            color: isFirst ? '#ddd' : '#999',
          }}
        >
          ▲
        </button>
        <button
          type="button"
          onClick={onMoveDown}
          disabled={isLast}
          style={{
            background: 'none',
            border: 'none',
            cursor: isLast ? 'default' : 'pointer',
            fontSize: '10px',
            padding: '0 2px',
            color: isLast ? '#ddd' : '#999',
          }}
        >
          ▼
        </button>
        <button
          type="button"
          onClick={onDelete}
          style={{
            background: 'none',
            border: 'none',
            color: '#c00',
            cursor: 'pointer',
            fontSize: '13px',
            padding: '0 2px',
            lineHeight: 1,
          }}
        >
          ×
        </button>
      </div>
      {expanded && (
        <div style={{ marginTop: '6px', display: 'flex', flexDirection: 'column', gap: '4px' }}>
          {(rule.filter?.conditions || []).map((cond, idx) => (
            <RuleConditionEditor
              key={idx}
              fields={fields}
              condition={cond}
              onChange={(c) => {
                const conds = [...(rule.filter?.conditions || [])];
                conds[idx] = c;
                updateConditions(conds);
              }}
              onRemove={() => {
                const conds = (rule.filter?.conditions || []).filter((_, i) => i !== idx);
                updateConditions(conds);
              }}
            />
          ))}
          <button
            type="button"
            onClick={() => {
              const conds = [...(rule.filter?.conditions || [])];
              if (fields.length > 0) {
                conds.push({ field: fields[0].name, operator: '==', value: '' });
              }
              updateConditions(conds);
            }}
            style={{
              fontSize: '11px',
              color: '#1976d2',
              background: 'none',
              border: '1px dashed #ccc',
              borderRadius: '3px',
              padding: '3px',
              cursor: 'pointer',
              width: '100%',
            }}
          >
            + {t('map.addCondition')}
          </button>
        </div>
      )}
    </div>
  );
}

function RulesSection({ sourceId, layerType, renderer, onChange }) {
  const { t } = useTranslation();
  const [fields, setFields] = useState([]);
  const rules = renderer.rules || [];
  const elseColor = renderer.elseColor || '#cccccc';
  const opacity = renderer.opacity ?? 0.7;

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

  function updateRule(idx, updatedRule) {
    const newRules = [...rules];
    newRules[idx] = updatedRule;
    onChange({ rules: newRules, elseColor, opacity });
  }

  function addRule() {
    const newRules = [
      ...rules,
      {
        filter: {
          conditions:
            fields.length > 0 ? [{ field: fields[0].name, operator: '==', value: '' }] : [],
        },
        color: '#0080ff',
        enabled: true,
      },
    ];
    onChange({ rules: newRules, elseColor, opacity });
  }

  function addElseRule() {
    const newRules = [...rules, { filter: { conditions: [] }, color: elseColor, enabled: true }];
    onChange({ rules: newRules, elseColor, opacity });
  }

  function deleteRule(idx) {
    const newRules = rules.filter((_, i) => i !== idx);
    onChange({ rules: newRules, elseColor, opacity });
  }

  function moveRule(idx, dir) {
    const newIdx = idx + dir;
    if (newIdx < 0 || newIdx >= rules.length) return;
    const newRules = [...rules];
    [newRules[idx], newRules[newIdx]] = [newRules[newIdx], newRules[idx]];
    onChange({ rules: newRules, elseColor, opacity });
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      {rules.map((rule, idx) => (
        <RuleEditor
          key={idx}
          rule={rule}
          fields={fields}
          layerType={layerType}
          onChange={(r) => updateRule(idx, r)}
          onDelete={() => deleteRule(idx)}
          onMoveUp={() => moveRule(idx, -1)}
          onMoveDown={() => moveRule(idx, 1)}
          isFirst={idx === 0}
          isLast={idx === rules.length - 1}
        />
      ))}

      <div style={{ display: 'flex', gap: '4px' }}>
        <button
          type="button"
          onClick={addRule}
          style={{
            flex: 1,
            fontSize: '11px',
            color: '#1976d2',
            background: 'none',
            border: '1px dashed #ccc',
            borderRadius: '3px',
            padding: '4px',
            cursor: 'pointer',
          }}
        >
          + {t('map.addRule')}
        </button>
        {!rules.some((r) => !r.filter?.conditions?.length) && (
          <button
            type="button"
            onClick={addElseRule}
            style={{
              flex: 1,
              fontSize: '11px',
              color: '#666',
              background: 'none',
              border: '1px dashed #ccc',
              borderRadius: '3px',
              padding: '4px',
              cursor: 'pointer',
            }}
          >
            + {t('map.addElseRule')}
          </button>
        )}
      </div>

      {rules.length > 0 && (
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <Label>{t('map.elseColor')}</Label>
          <input
            type="color"
            value={elseColor}
            onChange={(e) => onChange({ rules, elseColor: e.target.value, opacity })}
            style={{
              width: '32px',
              height: '24px',
              border: '1px solid #ddd',
              borderRadius: '3px',
              cursor: 'pointer',
            }}
          />
        </div>
      )}

      <div>
        <Label>
          {t('map.opacity')}: {Math.round(opacity * 100)}%
        </Label>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={opacity}
          onChange={(e) => onChange({ rules, elseColor, opacity: parseFloat(e.target.value) })}
          style={{ width: '100%' }}
        />
      </div>

      {rules.length === 0 && (
        <div style={{ fontSize: '11px', color: '#999' }}>{t('map.noRules')}</div>
      )}
    </div>
  );
}

const FONT_OPTIONS = [
  { value: 'Arial', label: 'Arial' },
  { value: 'Helvetica', label: 'Helvetica' },
  { value: 'sans-serif', label: 'Sans Serif' },
  { value: 'serif', label: 'Serif' },
  { value: 'monospace', label: 'Monospace' },
];

const LABEL_PLACEMENTS = [
  { value: 'centroid', label: 'Centroid' },
  { value: 'line', label: 'Along Line' },
  { value: 'point', label: 'Point' },
];

function LabelSection({ sourceId, layerType, label, onChange }) {
  const { t } = useTranslation();
  const [fields, setFields] = useState([]);
  const enabled = label?.enabled ?? false;
  const field = label?.field || '';
  const font = label?.font || 'Arial';
  const size = label?.size ?? 12;
  const color = label?.color || '#333333';
  const haloColor = label?.haloColor || '#ffffff';
  const haloWidth = label?.haloWidth ?? 1;
  const placement = label?.placement || 'centroid';
  const offsetX = label?.offsetX ?? 0;
  const offsetY = label?.offsetY ?? -12;

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

  function update(updates) {
    onChange({
      label: {
        ...label,
        enabled,
        field,
        font,
        size,
        color,
        haloColor,
        haloWidth,
        placement,
        offsetX,
        offsetY,
        ...updates,
      },
    });
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
        <Label>{t('map.showLabel')}</Label>
        <input
          type="checkbox"
          checked={enabled}
          onChange={(e) => update({ enabled: e.target.checked })}
        />
      </div>

      {enabled && (
        <>
          <div>
            <Label>{t('map.labelField')}</Label>
            <select
              value={field}
              onChange={(e) => update({ field: e.target.value })}
              style={{ fontSize: '12px', width: '100%', padding: '4px' }}
            >
              <option value="">{t('map.selectField')}</option>
              {fields.map((f) => (
                <option key={f.name} value={f.name}>
                  {f.alias || f.name}
                </option>
              ))}
            </select>
          </div>

          <div>
            <Label>{t('map.labelFont')}</Label>
            <select
              value={font}
              onChange={(e) => update({ font: e.target.value })}
              style={{ fontSize: '12px', width: '100%', padding: '4px' }}
            >
              {FONT_OPTIONS.map((f) => (
                <option key={f.value} value={f.value}>
                  {f.label}
                </option>
              ))}
            </select>
          </div>

          <div>
            <Label>
              {t('map.labelSize')}: {size}px
            </Label>
            <input
              type="range"
              min={6}
              max={48}
              step={1}
              value={size}
              onChange={(e) => update({ size: parseInt(e.target.value, 10) })}
              style={{ width: '100%' }}
            />
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
            <Label>{t('map.labelColor')}</Label>
            <input
              type="color"
              value={color}
              onChange={(e) => update({ color: e.target.value })}
              style={{
                width: '32px',
                height: '24px',
                border: '1px solid #ddd',
                borderRadius: '3px',
                cursor: 'pointer',
              }}
            />
          </div>

          <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
            <Label>{t('map.labelHalo')}</Label>
            <input
              type="color"
              value={haloColor}
              onChange={(e) => update({ haloColor: e.target.value })}
              style={{
                width: '32px',
                height: '24px',
                border: '1px solid #ddd',
                borderRadius: '3px',
                cursor: 'pointer',
              }}
            />
            <input
              type="number"
              min={0}
              max={5}
              step={0.5}
              value={haloWidth}
              onChange={(e) => update({ haloWidth: parseFloat(e.target.value) || 0 })}
              style={{ fontSize: '12px', width: '45px', padding: '3px 4px' }}
            />
          </div>

          <div>
            <Label>{t('map.labelPlacement')}</Label>
            <div style={{ display: 'flex', gap: '4px' }}>
              {LABEL_PLACEMENTS.map((p) => (
                <button
                  key={p.value}
                  type="button"
                  onClick={() => update({ placement: p.value })}
                  style={{
                    flex: 1,
                    padding: '4px 2px',
                    fontSize: '11px',
                    border: placement === p.value ? '2px solid #1976d2' : '1px solid #ddd',
                    borderRadius: '3px',
                    background: placement === p.value ? '#e3f2fd' : '#fff',
                    cursor: 'pointer',
                  }}
                >
                  {p.label}
                </button>
              ))}
            </div>
          </div>

          <div style={{ display: 'flex', gap: '8px' }}>
            <div style={{ flex: 1 }}>
              <Label>{t('map.labelOffsetX')}</Label>
              <input
                type="number"
                value={offsetX}
                onChange={(e) => update({ offsetX: parseInt(e.target.value, 10) || 0 })}
                style={{ fontSize: '12px', width: '100%', padding: '4px', boxSizing: 'border-box' }}
              />
            </div>
            <div style={{ flex: 1 }}>
              <Label>{t('map.labelOffsetY')}</Label>
              <input
                type="number"
                value={offsetY}
                onChange={(e) => update({ offsetY: parseInt(e.target.value, 10) || 0 })}
                style={{ fontSize: '12px', width: '100%', padding: '4px', boxSizing: 'border-box' }}
              />
            </div>
          </div>
        </>
      )}
    </div>
  );
}

function Label({ children }) {
  return <div style={{ fontSize: '11px', color: '#666', marginBottom: '4px' }}>{children}</div>;
}

function defaultRenderer(paint, layerType) {
  if (!paint || Object.keys(paint).length === 0) {
    return {
      type: 'single',
      color: layerType === 'circle' ? '#ff0040' : '#0080ff',
      opacity: layerType === 'fill' ? 0.7 : 1,
    };
  }

  for (const val of Object.values(paint)) {
    if (Array.isArray(val)) {
      if (val[0] === 'match') return { type: 'categorized', color: '#0080ff', opacity: 0.7 };
      if (val[0] === 'case' || val[0] === 'interpolate')
        return { type: 'graduated', color: '#0080ff', opacity: 0.7 };
    }
  }

  const color =
    paint['fill-color'] ||
    paint['line-color'] ||
    paint['circle-color'] ||
    (layerType === 'circle' ? '#ff0040' : '#0080ff');
  const opacity =
    paint['fill-opacity'] ??
    paint['line-opacity'] ??
    paint['circle-opacity'] ??
    (layerType === 'fill' ? 0.7 : 1);
  return { type: 'single', color, opacity };
}

function SingleColorSection({ layerType, renderer, onChange }) {
  const { t } = useTranslation();
  const color = renderer.color || (layerType === 'circle' ? '#ff0040' : '#0080ff');
  const opacity = renderer.opacity ?? (layerType === 'fill' ? 0.7 : 1);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
        <Label>{t('map.color')}</Label>
        <input
          type="color"
          value={color}
          onChange={(e) => onChange({ color: e.target.value })}
          style={{
            width: '32px',
            height: '24px',
            border: '1px solid #ddd',
            borderRadius: '3px',
            cursor: 'pointer',
          }}
        />
        <span style={{ fontSize: '11px', color: '#888', fontFamily: 'monospace' }}>{color}</span>
      </div>
      {layerType === 'circle' && (
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <Label>{t('map.circleRadius')}</Label>
          <input
            type="number"
            min={1}
            max={50}
            value={renderer.radius ?? 6}
            onChange={(e) => onChange({ radius: parseInt(e.target.value, 10) || 6 })}
            style={{ fontSize: '12px', width: '50px', padding: '3px 4px' }}
          />
        </div>
      )}
      {layerType === 'line' && (
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <Label>{t('map.lineWidth')}</Label>
          <input
            type="number"
            min={0.5}
            max={20}
            step={0.5}
            value={renderer.width ?? 2}
            onChange={(e) => onChange({ width: parseFloat(e.target.value) || 2 })}
            style={{ fontSize: '12px', width: '50px', padding: '3px 4px' }}
          />
        </div>
      )}
      <div>
        <Label>
          {t('map.opacity')}: {Math.round(opacity * 100)}%
        </Label>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={opacity}
          onChange={(e) => onChange({ opacity: parseFloat(e.target.value) })}
          style={{ width: '100%' }}
        />
      </div>
    </div>
  );
}

function CategorizedSection({ sourceId, layerType, renderer, onChange }) {
  const { t } = useTranslation();
  const [fields, setFields] = useState([]);
  const [selectedField, setSelectedField] = useState(renderer.field || '');
  const [fieldData, setFieldData] = useState(null);
  const [rampName, setRampName] = useState('Set1');
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
    if (!sourceId || !selectedField) return;
    let cancelled = false;
    setIsLoading(true);
    getFieldValues(sourceId, selectedField, 50)
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
  }, [sourceId, selectedField]);

  useEffect(() => {
    if (!fieldData?.values || !selectedField) return;
    const values = fieldData.values.map((v) => (typeof v === 'string' ? v : String(v)));
    const colors = resolveRampColors(rampName, values.length);
    const classes = values.map((v, i) => ({ value: v, color: colors[i] || '#888888' }));
    const opacity = renderer.opacity ?? 0.7;
    onChange({ field: selectedField, classes, opacity });
  }, [selectedField, fieldData, rampName]);

  const opacity = renderer.opacity ?? 0.7;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div>
        <Label>{t('map.classifyField')}</Label>
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
          <Label>{t('map.colorRamp')}</Label>
          <ColorRampSelector mode="categorical" value={rampName} onChange={setRampName} />
        </div>
      )}

      {isLoading && <span style={{ fontSize: '11px', color: '#888' }}>{t('common.loading')}</span>}

      {fieldData?.values && (
        <div style={{ maxHeight: '120px', overflow: 'auto', fontSize: '11px', color: '#555' }}>
          {fieldData.values.length} {t('map.classes')}
        </div>
      )}

      <div>
        <Label>
          {t('map.opacity')}: {Math.round(opacity * 100)}%
        </Label>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={opacity}
          onChange={(e) => onChange({ opacity: parseFloat(e.target.value) })}
          style={{ width: '100%' }}
        />
      </div>
    </div>
  );
}

function GraduatedSection({ sourceId, layerType, renderer, onChange }) {
  const { t } = useTranslation();
  const [fields, setFields] = useState([]);
  const [selectedField, setSelectedField] = useState(renderer.field || '');
  const [fieldData, setFieldData] = useState(null);
  const [rampName, setRampName] = useState('Blues');
  const [classCount, setClassCount] = useState(renderer.stops?.length || 5);
  const [method, setMethod] = useState(renderer.method || 'equal');
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
                x.type === 'Integer' ||
                x.type === 'Real' ||
                x.type === 'Double' ||
                x.type === 'Float' ||
                x.type === 'BigInt' ||
                x.type === 'SmallInt',
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
    if (!sourceId || !selectedField) return;
    let cancelled = false;
    setIsLoading(true);
    getFieldValues(sourceId, selectedField, 0)
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
  }, [sourceId, selectedField]);

  useEffect(() => {
    if (fieldData?.min == null || !selectedField) return;
    const count = Math.max(2, classCount);
    const colors = resolveRampColors(rampName, count);
    let breaks;

    const sorted = fieldData.sortedValues || [];
    if (method === 'quantile' && sorted.length > 0) {
      breaks = quantileBreaks(sorted, count);
    } else if (method === 'jenks' && sorted.length > 0) {
      breaks = jenksBreaks(sorted, count);
    } else {
      breaks = equalIntervalBreaks(fieldData.min, fieldData.max, count);
    }

    const stops = breaks.map((b, i) => ({ value: b, color: colors[i] }));
    const opacity = renderer.opacity ?? 0.7;
    onChange({ field: selectedField, stops, opacity, method });
  }, [selectedField, fieldData, rampName, classCount, method]);

  const opacity = renderer.opacity ?? 0.7;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div>
        <Label>{t('map.classifyField')}</Label>
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
        <>
          <div>
            <Label>{t('map.classMethod')}</Label>
            <select
              value={method}
              onChange={(e) => setMethod(e.target.value)}
              style={{ fontSize: '12px', width: '100%', padding: '4px' }}
            >
              <option value="equal">{t('map.methodEqual')}</option>
              <option value="quantile">{t('map.methodQuantile')}</option>
              <option value="jenks">{t('map.methodJenks')}</option>
            </select>
          </div>
          <div>
            <Label>{t('map.colorRamp')}</Label>
            <ColorRampSelector mode="sequential" value={rampName} onChange={setRampName} />
          </div>
          <div>
            <Label>{t('map.classCount')}</Label>
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
        </>
      )}

      {isLoading && <span style={{ fontSize: '11px', color: '#888' }}>{t('common.loading')}</span>}

      <div>
        <Label>
          {t('map.opacity')}: {Math.round(opacity * 100)}%
        </Label>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={opacity}
          onChange={(e) => onChange({ opacity: parseFloat(e.target.value) })}
          style={{ width: '100%' }}
        />
      </div>
    </div>
  );
}

function ProportionalSection({ sourceId, renderer, onChange }) {
  const { t } = useTranslation();
  const [fields, setFields] = useState([]);
  const [selectedField, setSelectedField] = useState(renderer.field || '');
  const [fieldData, setFieldData] = useState(null);
  const [minRadius, setMinRadius] = useState(renderer.minRadius ?? 3);
  const [maxRadius, setMaxRadius] = useState(renderer.maxRadius ?? 25);
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
                x.type === 'Integer' ||
                x.type === 'Real' ||
                x.type === 'Double' ||
                x.type === 'Float' ||
                x.type === 'BigInt' ||
                x.type === 'SmallInt',
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
    if (!sourceId || !selectedField) return;
    let cancelled = false;
    setIsLoading(true);
    getFieldValues(sourceId, selectedField, 0)
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
  }, [sourceId, selectedField]);

  useEffect(() => {
    if (!fieldData || fieldData.min == null || !selectedField) return;
    onChange({
      field: selectedField,
      minRadius,
      maxRadius,
      minVal: fieldData.min,
      maxVal: fieldData.max,
      color: renderer.color || '#ff0040',
      opacity: renderer.opacity ?? 0.8,
    });
  }, [selectedField, fieldData, minRadius, maxRadius]);

  const color = renderer.color || '#ff0040';
  const opacity = renderer.opacity ?? 0.8;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div>
        <Label>{t('map.classifyField')}</Label>
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

      {selectedField && fieldData?.min != null && (
        <>
          <div style={{ display: 'flex', gap: '8px' }}>
            <div style={{ flex: 1 }}>
              <Label>
                {t('map.minRadius')}: {minRadius}px
              </Label>
              <input
                type="range"
                min={1}
                max={30}
                step={1}
                value={minRadius}
                onChange={(e) => setMinRadius(parseInt(e.target.value, 10))}
                style={{ width: '100%' }}
              />
            </div>
            <div style={{ flex: 1 }}>
              <Label>
                {t('map.maxRadius')}: {maxRadius}px
              </Label>
              <input
                type="range"
                min={5}
                max={50}
                step={1}
                value={maxRadius}
                onChange={(e) => setMaxRadius(parseInt(e.target.value, 10))}
                style={{ width: '100%' }}
              />
            </div>
          </div>
          <div style={{ fontSize: '11px', color: '#888' }}>
            {fieldData.min.toFixed(2)} – {fieldData.max.toFixed(2)}
          </div>
        </>
      )}

      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
        <Label>{t('map.color')}</Label>
        <input
          type="color"
          value={color}
          onChange={(e) => onChange({ color: e.target.value })}
          style={{
            width: '32px',
            height: '24px',
            border: '1px solid #ddd',
            borderRadius: '3px',
            cursor: 'pointer',
          }}
        />
      </div>

      {isLoading && <span style={{ fontSize: '11px', color: '#888' }}>{t('common.loading')}</span>}

      <div>
        <Label>
          {t('map.opacity')}: {Math.round(opacity * 100)}%
        </Label>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={opacity}
          onChange={(e) => onChange({ opacity: parseFloat(e.target.value) })}
          style={{ width: '100%' }}
        />
      </div>
    </div>
  );
}

function LineStyleSection({ renderer, onChange }) {
  const { t } = useTranslation();
  const lineStyle = renderer.lineStyle || 'solid';
  const lineCap = renderer.lineCap || 'butt';
  const lineJoin = renderer.lineJoin || 'mitre';

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div>
        <Label>{t('map.lineDash')}</Label>
        <div style={{ display: 'flex', gap: '4px' }}>
          {LINE_STYLES.map((ls) => (
            <button
              key={ls.value}
              type="button"
              onClick={() => onChange({ lineStyle: ls.value })}
              style={{
                flex: 1,
                padding: '4px 2px',
                fontSize: '11px',
                fontFamily: 'monospace',
                border: lineStyle === ls.value ? '2px solid #1976d2' : '1px solid #ddd',
                borderRadius: '3px',
                background: lineStyle === ls.value ? '#e3f2fd' : '#fff',
                cursor: 'pointer',
              }}
            >
              {ls.label}
            </button>
          ))}
        </div>
      </div>
      <div>
        <Label>{t('map.lineCap')}</Label>
        <div style={{ display: 'flex', gap: '4px' }}>
          {LINE_CAPS.map((cap) => (
            <button
              key={cap.value}
              type="button"
              onClick={() => onChange({ lineCap: cap.value })}
              style={{
                flex: 1,
                padding: '4px 2px',
                fontSize: '11px',
                border: lineCap === cap.value ? '2px solid #1976d2' : '1px solid #ddd',
                borderRadius: '3px',
                background: lineCap === cap.value ? '#e3f2fd' : '#fff',
                cursor: 'pointer',
              }}
            >
              {cap.label}
            </button>
          ))}
        </div>
      </div>
      <div>
        <Label>{t('map.lineJoin')}</Label>
        <div style={{ display: 'flex', gap: '4px' }}>
          {LINE_JOINS.map((join) => (
            <button
              key={join.value}
              type="button"
              onClick={() => onChange({ lineJoin: join.value })}
              style={{
                flex: 1,
                padding: '4px 2px',
                fontSize: '11px',
                border: lineJoin === join.value ? '2px solid #1976d2' : '1px solid #ddd',
                borderRadius: '3px',
                background: lineJoin === join.value ? '#e3f2fd' : '#fff',
                cursor: 'pointer',
              }}
            >
              {join.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

const FILTER_OPERATORS = [
  { value: '==', label: '=' },
  { value: '!=', label: '≠' },
  { value: '>', label: '>' },
  { value: '<', label: '<' },
  { value: '>=', label: '≥' },
  { value: '<=', label: '≤' },
  { value: 'contains', label: 'contains' },
];

function ScaleSection({ minzoom, maxzoom, onChange }) {
  const { t } = useTranslation();

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
        <div style={{ flex: 1 }}>
          <Label>{t('map.minZoom')}</Label>
          <input
            type="number"
            min={0}
            max={22}
            value={minzoom ?? ''}
            placeholder="0"
            onChange={(e) => {
              const v =
                e.target.value === ''
                  ? undefined
                  : Math.max(0, Math.min(22, parseInt(e.target.value, 10)));
              onChange({ minzoom: v });
            }}
            style={{ fontSize: '12px', width: '100%', padding: '4px', boxSizing: 'border-box' }}
          />
        </div>
        <span style={{ fontSize: '12px', color: '#999', marginTop: '14px' }}>–</span>
        <div style={{ flex: 1 }}>
          <Label>{t('map.maxZoom')}</Label>
          <input
            type="number"
            min={0}
            max={22}
            value={maxzoom ?? ''}
            placeholder="22"
            onChange={(e) => {
              const v =
                e.target.value === ''
                  ? undefined
                  : Math.max(0, Math.min(22, parseInt(e.target.value, 10)));
              onChange({ maxzoom: v });
            }}
            style={{ fontSize: '12px', width: '100%', padding: '4px', boxSizing: 'border-box' }}
          />
        </div>
      </div>
      <div style={{ fontSize: '10px', color: '#999' }}>{t('map.scaleHint')}</div>
    </div>
  );
}

function FilterSection({ sourceId, filter, onChange }) {
  const { t } = useTranslation();
  const [fields, setFields] = useState([]);
  const [conditions, setConditions] = useState(() => {
    if (!filter?.conditions?.length) return [];
    return filter.conditions;
  });

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
    if (conditions.length === 0) {
      onChange({ filter: undefined });
      return;
    }
    onChange({ filter: { conditions } });
  }, [conditions]);

  function updateCondition(idx, key, value) {
    setConditions((prev) => prev.map((c, i) => (i === idx ? { ...c, [key]: value } : c)));
  }

  function addCondition() {
    if (fields.length === 0) return;
    setConditions((prev) => [...prev, { field: fields[0].name, operator: '==', value: '' }]);
  }

  function removeCondition(idx) {
    setConditions((prev) => prev.filter((_, i) => i !== idx));
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
      {conditions.map((cond, idx) => (
        <div
          key={idx}
          style={{ display: 'flex', gap: '4px', alignItems: 'center', flexWrap: 'wrap' }}
        >
          <select
            value={cond.field}
            onChange={(e) => updateCondition(idx, 'field', e.target.value)}
            style={{ fontSize: '11px', flex: '1 1 70px', minWidth: '60px', padding: '2px' }}
          >
            {fields.map((f) => (
              <option key={f.name} value={f.name}>
                {f.alias || f.name}
              </option>
            ))}
          </select>
          <select
            value={cond.operator}
            onChange={(e) => updateCondition(idx, 'operator', e.target.value)}
            style={{ fontSize: '11px', width: '65px', padding: '2px' }}
          >
            {FILTER_OPERATORS.map((op) => (
              <option key={op.value} value={op.value}>
                {op.label}
              </option>
            ))}
          </select>
          <input
            type="text"
            value={cond.value ?? ''}
            placeholder="value"
            onChange={(e) => updateCondition(idx, 'value', e.target.value)}
            style={{ fontSize: '11px', flex: '1 1 50px', minWidth: '40px', padding: '2px 4px' }}
          />
          <button
            type="button"
            onClick={() => removeCondition(idx)}
            style={{
              background: 'none',
              border: 'none',
              color: '#c00',
              cursor: 'pointer',
              fontSize: '14px',
              padding: '0 2px',
              lineHeight: 1,
            }}
          >
            ×
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={addCondition}
        style={{
          fontSize: '11px',
          color: '#1976d2',
          background: 'none',
          border: '1px dashed #ccc',
          borderRadius: '3px',
          padding: '4px',
          cursor: 'pointer',
          width: '100%',
        }}
      >
        + {t('map.addCondition')}
      </button>
      {conditions.length === 0 && (
        <div style={{ fontSize: '11px', color: '#999' }}>{t('map.noFilter')}</div>
      )}
    </div>
  );
}

export default function LayerStylePanel({
  sourceId,
  layerType,
  paint,
  renderer: rendererProp,
  layerMeta,
  onRendererChange,
  onMetaChange,
}) {
  const { t } = useTranslation();

  const [renderer, setRenderer] = useState(() => {
    return rendererProp || defaultRenderer(paint, layerType);
  });

  useEffect(() => {
    setRenderer(rendererProp || defaultRenderer(paint, layerType));
  }, [rendererProp, paint, layerType]);

  const rendererType = renderer.type || 'single';

  const handleRendererTypeChange = useCallback(
    (newType) => {
      let updated = {
        type: newType,
        color: renderer.color,
        opacity: renderer.opacity ?? (layerType === 'fill' ? 0.7 : 1),
      };
      if (newType === 'none') {
        updated.color = undefined;
        updated.opacity = undefined;
      }
      if (newType === 'rules') {
        updated.rules = [];
        updated.elseColor = '#cccccc';
      }
      setRenderer(updated);
      onRendererChange(updated);
    },
    [renderer, onRendererChange, layerType],
  );

  const handleSymbolChange = useCallback(
    (updates) => {
      const updated = { ...renderer, ...updates };
      setRenderer(updated);
      onRendererChange(updated);
    },
    [renderer, onRendererChange],
  );

  const handleLineChange = useCallback(
    (updates) => {
      const updated = { ...renderer, ...updates };
      setRenderer(updated);
      onRendererChange(updated);
    },
    [renderer, onRendererChange],
  );

  const showLineSettings = layerType === 'line';

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
      <div style={{ marginBottom: '6px' }}>
        <Label>{t('map.renderer')}</Label>
        <div style={{ display: 'flex', gap: '3px' }}>
          {RENDERER_TYPES.map((rt) => {
            const labelKey = 'map.renderer_' + rt.value;
            const isSelected = rendererType === rt.value;
            return (
              <button
                key={rt.value}
                type="button"
                onClick={() => handleRendererTypeChange(rt.value)}
                style={{
                  flex: 1,
                  padding: '6px 2px',
                  fontSize: '11px',
                  border: isSelected ? '2px solid #1976d2' : '1px solid #ddd',
                  borderRadius: '4px',
                  background: isSelected ? '#e3f2fd' : '#fff',
                  cursor: 'pointer',
                  textAlign: 'center',
                }}
              >
                <div style={{ fontSize: '16px' }}>{rt.icon}</div>
                <div style={{ marginTop: '2px', color: '#555' }}>{t(labelKey)}</div>
              </button>
            );
          })}
        </div>
      </div>

      <CollapsibleSection title={t('map.sectionSymbol')}>
        {rendererType === 'none' && (
          <div style={{ fontSize: '12px', color: '#888', padding: '4px 0' }}>
            {t('map.noSymbol')}
          </div>
        )}
        {rendererType === 'single' && (
          <SingleColorSection
            layerType={layerType}
            renderer={renderer}
            onChange={handleSymbolChange}
          />
        )}
        {rendererType === 'categorized' && (
          <CategorizedSection
            sourceId={sourceId}
            layerType={layerType}
            renderer={renderer}
            onChange={handleSymbolChange}
          />
        )}
        {rendererType === 'graduated' && (
          <GraduatedSection
            sourceId={sourceId}
            layerType={layerType}
            renderer={renderer}
            onChange={handleSymbolChange}
          />
        )}
        {rendererType === 'proportional' && (
          <ProportionalSection
            sourceId={sourceId}
            renderer={renderer}
            onChange={handleSymbolChange}
          />
        )}
        {rendererType === 'rules' && (
          <RulesSection
            sourceId={sourceId}
            layerType={layerType}
            renderer={renderer}
            onChange={handleSymbolChange}
          />
        )}
      </CollapsibleSection>

      {showLineSettings && (
        <CollapsibleSection title={t('map.sectionLineStyle')} defaultOpen={rendererType !== 'none'}>
          {rendererType === 'none' ? (
            <div style={{ fontSize: '12px', color: '#aaa', padding: '4px 0' }}>
              {t('map.noSymbol')}
            </div>
          ) : (
            <LineStyleSection renderer={renderer} onChange={handleLineChange} />
          )}
        </CollapsibleSection>
      )}

      <CollapsibleSection title={t('map.sectionLabel')} defaultOpen={false}>
        <LabelSection
          sourceId={sourceId}
          layerType={layerType}
          label={layerMeta?.label}
          onChange={onMetaChange}
        />
      </CollapsibleSection>

      <CollapsibleSection title={t('map.sectionScale')} defaultOpen={false}>
        <ScaleSection
          minzoom={layerMeta?.minzoom}
          maxzoom={layerMeta?.maxzoom}
          onChange={onMetaChange}
        />
      </CollapsibleSection>

      <CollapsibleSection title={t('map.sectionFilter')} defaultOpen={false}>
        <FilterSection sourceId={sourceId} filter={layerMeta?.filter} onChange={onMetaChange} />
      </CollapsibleSection>
    </div>
  );
}
