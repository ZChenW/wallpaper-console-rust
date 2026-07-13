import { LayoutGrid, Star, FolderCog, Settings } from 'lucide-react';

type View = 'library' | 'favorites' | 'sources';

interface Props {
  view: View;
  settingsOpen: boolean;
  onNavigate: (v: View) => void;
  onOpenSettings: () => void;
}

type SidebarItem =
  | { id: View; label: string; Icon: typeof LayoutGrid; kind: 'view' }
  | { id: 'settings'; label: string; Icon: typeof LayoutGrid; kind: 'settings' };

const items: SidebarItem[] = [
  { id: 'library', label: 'Library', Icon: LayoutGrid, kind: 'view' },
  { id: 'favorites', label: 'Favorites', Icon: Star, kind: 'view' },
  { id: 'sources', label: 'Sources', Icon: FolderCog, kind: 'view' },
  { id: 'settings', label: 'Settings', Icon: Settings, kind: 'settings' },
];

export default function Sidebar({ view, settingsOpen, onNavigate, onOpenSettings }: Props) {
  return (
    <nav className="sidebar">
      {items.map((item) => {
        const active = item.kind === 'settings' ? settingsOpen : view === item.id;
        return (
          <button
            key={item.id}
            className={`sidebar-btn ${active ? 'active' : ''}`}
            onClick={() => item.kind === 'settings' ? onOpenSettings() : onNavigate(item.id)}
            title={item.label}
            aria-current={active ? 'page' : undefined}
          >
            <item.Icon size={20} />
            <span className="sidebar-label">{item.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
