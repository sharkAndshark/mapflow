export function formatInspectorValue(value) {
  if (value === null) {
    return { text: '--', title: 'NULL', tone: 'null' };
  }

  if (typeof value === 'string' && value.length === 0) {
    return { text: '""', title: 'Empty string', tone: 'empty' };
  }

  return { text: String(value), title: undefined, tone: 'value' };
}
