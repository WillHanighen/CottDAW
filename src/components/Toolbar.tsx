import React, { useRef } from 'react';
import { PenTool, Eraser, MousePointer2, Save, FolderOpen, Download } from 'lucide-react';
import Button from './ui/Button.tsx';
import Select from './ui/Select.tsx';
import { useUIStore } from '../stores/uiStore.ts';
import { useProjectStore } from '../stores/projectStore.ts';
import type { Tool, GridSnap } from '../types/index.ts';

const GRID_OPTIONS: { value: GridSnap; label: string }[] = [
  { value: '1/4', label: '1/4' },
  { value: '1/8', label: '1/8' },
  { value: '1/16', label: '1/16' },
  { value: '1/32', label: '1/32' },
];

export default function Toolbar() {
  const currentTool = useUIStore((s) => s.currentTool);
  const setTool = useUIStore((s) => s.setTool);
  const gridSnap = useUIStore((s) => s.gridSnap);
  const setGridSnap = useUIStore((s) => s.setGridSnap);
  const zoomIn = useUIStore((s) => s.zoomIn);
  const zoomOut = useUIStore((s) => s.zoomOut);
  const horizontalZoom = useUIStore((s) => s.horizontalZoom);

  const exportProject = useProjectStore((s) => s.exportProject);
  const importProject = useProjectStore((s) => s.importProject);
  const projectName = useProjectStore((s) => s.projectName);
  const setProjectName = useProjectStore((s) => s.setProjectName);

  const fileInputRef = useRef<HTMLInputElement>(null);

  const tools: { tool: Tool; icon: React.ReactNode; label: string }[] = [
    { tool: 'draw', icon: <PenTool size={16} />, label: 'Draw (D)' },
    { tool: 'eraser', icon: <Eraser size={16} />, label: 'Eraser (E)' },
    { tool: 'select', icon: <MousePointer2 size={16} />, label: 'Select (V)' },
  ];

  const handleSave = () => {
    const project = exportProject();
    const blob = new Blob([JSON.stringify(project, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${projectName.replace(/[^a-z0-9]/gi, '_').toLowerCase()}.cottdaw.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleLoad = () => {
    fileInputRef.current?.click();
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = (event) => {
      try {
        const project = JSON.parse(event.target?.result as string);
        importProject(project);
      } catch (err) {
        console.error('Failed to parse project file:', err);
        alert('Invalid project file');
      }
    };
    reader.readAsText(file);
    e.target.value = '';
  };

  // Keyboard shortcuts
  React.useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      
      switch (e.key.toLowerCase()) {
        case 'd':
          setTool('draw');
          break;
        case 'e':
          setTool('eraser');
          break;
        case 'v':
          setTool('select');
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [setTool]);

  return (
    <div className="flex items-center justify-between px-4 py-2 bg-[var(--color-bg-secondary)]">
      {/* Left: Project name & Tools */}
      <div className="flex items-center gap-6">
        {/* Project Name */}
        <input
          type="text"
          value={projectName}
          onChange={(e) => setProjectName(e.target.value)}
          className="bg-transparent border-b border-transparent hover:border-[var(--color-border)] focus:border-[var(--color-accent)] px-1 py-0.5 text-lg font-medium text-[var(--color-text-primary)] focus:outline-none"
        />

        {/* Tools */}
        <div className="flex items-center gap-1 p-1 bg-[var(--color-bg-tertiary)] rounded-lg">
          {tools.map(({ tool, icon, label }) => (
            <Button
              key={tool}
              variant="ghost"
              size="sm"
              active={currentTool === tool}
              onClick={() => setTool(tool)}
              title={label}
              className="tool-btn"
            >
              {icon}
            </Button>
          ))}
        </div>

        {/* Grid Snap */}
        <Select
          value={gridSnap}
          options={GRID_OPTIONS}
          onChange={setGridSnap}
          label="Grid"
        />

        {/* Zoom */}
        <div className="flex items-center gap-2">
          <span className="text-xs text-[var(--color-text-secondary)]">Zoom</span>
          <Button variant="ghost" size="sm" onClick={zoomOut}>−</Button>
          <span className="text-xs font-mono w-12 text-center">{Math.round(horizontalZoom * 100)}%</span>
          <Button variant="ghost" size="sm" onClick={zoomIn}>+</Button>
        </div>
      </div>

      {/* Right: File Operations */}
      <div className="flex items-center gap-2">
        <Button variant="ghost" size="sm" onClick={handleSave} title="Save Project">
          <Save size={16} />
          <span>Save</span>
        </Button>
        <Button variant="ghost" size="sm" onClick={handleLoad} title="Load Project">
          <FolderOpen size={16} />
          <span>Load</span>
        </Button>
        <Button variant="default" size="sm" id="export-wav-btn" title="Export WAV">
          <Download size={16} />
          <span>Export WAV</span>
        </Button>
        <input
          ref={fileInputRef}
          type="file"
          accept=".json,.cottdaw.json"
          onChange={handleFileChange}
          className="hidden"
        />
      </div>
    </div>
  );
}

