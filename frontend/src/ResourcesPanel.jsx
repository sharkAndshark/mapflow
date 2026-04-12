import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import FontsPanel from './FontsPanel.jsx';
import IconsPanel from './IconsPanel.jsx';
import StylesPanel from './StylesPanel.jsx';

export default function ResourcesPanel() {
  const { t } = useTranslation();
  const [resourceTab, setResourceTab] = useState('fonts');

  return (
    <div role="tabpanel" id="main-tabpanel-resources">
      <div
        className="panel-tabs"
        role="tablist"
        style={{ borderBottom: '1px solid #e0e0e0', marginBottom: '12px' }}
      >
        <button
          type="button"
          className={`tab-btn ${resourceTab === 'fonts' ? 'active' : ''}`}
          onClick={() => setResourceTab('fonts')}
          role="tab"
          aria-selected={resourceTab === 'fonts'}
          aria-controls="resource-tabpanel-fonts"
          data-testid="resource-tab-fonts"
        >
          {t('app.resourceTabFonts')}
        </button>
        <button
          type="button"
          className={`tab-btn ${resourceTab === 'icons' ? 'active' : ''}`}
          onClick={() => setResourceTab('icons')}
          role="tab"
          aria-selected={resourceTab === 'icons'}
          aria-controls="resource-tabpanel-icons"
          data-testid="resource-tab-icons"
        >
          {t('app.resourceTabIcons')}
        </button>
        <button
          type="button"
          className={`tab-btn ${resourceTab === 'styles' ? 'active' : ''}`}
          onClick={() => setResourceTab('styles')}
          role="tab"
          aria-selected={resourceTab === 'styles'}
          aria-controls="resource-tabpanel-styles"
          data-testid="resource-tab-styles"
        >
          {t('app.resourceTabStyles')}
        </button>
      </div>

      {resourceTab === 'fonts' && (
        <div role="tabpanel" id="resource-tabpanel-fonts">
          <FontsPanel />
        </div>
      )}

      {resourceTab === 'icons' && (
        <div role="tabpanel" id="resource-tabpanel-icons">
          <IconsPanel />
        </div>
      )}

      {resourceTab === 'styles' && (
        <div role="tabpanel" id="resource-tabpanel-styles">
          <StylesPanel />
        </div>
      )}
    </div>
  );
}
