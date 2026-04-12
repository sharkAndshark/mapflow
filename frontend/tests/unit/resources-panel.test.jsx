/* @vitest-environment jsdom */

import React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import ResourcesPanel from '../../src/ResourcesPanel.jsx';

vi.mock('../../src/FontsPanel.jsx', () => ({
  default: function MockFontsPanel() {
    return <div data-testid="fonts-panel">fonts-panel</div>;
  },
}));

vi.mock('../../src/IconsPanel.jsx', () => ({
  default: function MockIconsPanel() {
    return <div data-testid="icons-panel">icons-panel</div>;
  },
}));

vi.mock('../../src/StylesPanel.jsx', () => ({
  default: function MockStylesPanel() {
    return <div data-testid="styles-panel">styles-panel</div>;
  },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key) => {
      const labels = {
        'app.resourceTabFonts': 'Fonts',
        'app.resourceTabIcons': 'Icon Sets',
        'app.resourceTabStyles': 'Styles',
      };
      return labels[key] || key;
    },
  }),
}));

afterEach(() => {
  cleanup();
});

describe('ResourcesPanel', () => {
  it('shows fonts tab content by default', () => {
    render(<ResourcesPanel />);

    expect(screen.getByTestId('fonts-panel')).toBeTruthy();
    expect(screen.queryByTestId('icons-panel')).toBeNull();
    expect(screen.queryByTestId('styles-panel')).toBeNull();
  });

  it('switches to icon sets and styles tabs', async () => {
    const user = userEvent.setup();
    render(<ResourcesPanel />);

    await user.click(screen.getByTestId('resource-tab-icons'));
    expect(screen.queryByTestId('fonts-panel')).toBeNull();
    expect(screen.getByTestId('icons-panel')).toBeTruthy();
    expect(screen.queryByTestId('styles-panel')).toBeNull();

    await user.click(screen.getByTestId('resource-tab-styles'));
    expect(screen.queryByTestId('fonts-panel')).toBeNull();
    expect(screen.queryByTestId('icons-panel')).toBeNull();
    expect(screen.getByTestId('styles-panel')).toBeTruthy();
  });

  it('updates aria-selected state when switching tabs', async () => {
    const user = userEvent.setup();
    render(<ResourcesPanel />);

    const fontsTab = screen.getByTestId('resource-tab-fonts');
    const iconsTab = screen.getByTestId('resource-tab-icons');
    const stylesTab = screen.getByTestId('resource-tab-styles');

    expect(fontsTab.getAttribute('aria-selected')).toBe('true');
    expect(iconsTab.getAttribute('aria-selected')).toBe('false');
    expect(stylesTab.getAttribute('aria-selected')).toBe('false');

    await user.click(iconsTab);
    expect(fontsTab.getAttribute('aria-selected')).toBe('false');
    expect(iconsTab.getAttribute('aria-selected')).toBe('true');
    expect(stylesTab.getAttribute('aria-selected')).toBe('false');
  });
});
