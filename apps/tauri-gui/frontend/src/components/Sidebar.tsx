import { LayoutGrid, Star, Clock, FolderCog, Settings } from 'lucide-react';

type View = 'library' | 'favorites' | 'history' | 'sources' | 'settings';

interface Props {
  view: View;
  onNavigate: (v: View) => void;
}

const items: { id: View; label: string; Icon: typeof LayoutGrid }[] = [
  { id: 'library', label: 'Library', Icon: LayoutGrid },
  { id: 'favorites', label: 'Favorites', Icon: Star },
  { id: 'history', label: 'History', Icon: Clock },
  { id: 'sources', label: 'Sources', Icon: FolderCog },
  { id: 'settings', label: 'Settings', Icon: Settings },
];

export default function Sidebar({ view, onNavigate }: Props) {
  return (
    <nav className="sidebar">
      {items.map(({ id, label, Icon }) => (
        <button
          key={id}
          className={`sidebar-btn ${view === id ? 'active' : ''}`}
          onClick={() => onNavigate(id)}
          title={label}
        >
          <Icon size={20} />
          <span className="sidebar-label">{label}</span>
        </button>
      ))}
    </nav>
  );
}
