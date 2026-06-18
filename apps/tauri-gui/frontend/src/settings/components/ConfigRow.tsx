import { useState, useEffect, useRef } from 'react';
import { Loader } from 'lucide-react';
import type { ConfigRowProps } from '../types';

export default function ConfigRow({ setting, value, saving, onSet }: ConfigRowProps) {
  const [edit, setEdit] = useState(value);
  const submitting = useRef(false);

  useEffect(() => { setEdit(value); }, [value]);

  const submit = async (v: string) => {
    if (submitting.current) return;
    submitting.current = true;
    setEdit(v);
    const ok = await onSet(v);
    if (!ok) setEdit(value);
    submitting.current = false;
  };

  return (
    <div className="config-row">
      <div className="config-info">
        <span className="config-label">{setting.label}</span>
        {setting.description && <span className="config-desc">{setting.description}</span>}
      </div>
      <div className="config-input">
        {setting.type === 'select' && setting.options ? (
          <select
            value={edit}
            onChange={(e) => submit(e.target.value)}
            disabled={saving}
          >
            {setting.options.map((o) => (
              <option key={o} value={o}>{setting.optionLabels?.[o] ?? o}</option>
            ))}
          </select>
        ) : (
          <input
            type={setting.type === 'number' ? 'number' : 'text'}
            value={edit}
            placeholder={setting.placeholder}
            onChange={(e) => setEdit(e.target.value)}
            onBlur={() => { if (edit !== value && !submitting.current) submit(edit); }}
            onKeyDown={(e) => { if (e.key === 'Enter' && edit !== value && !submitting.current) submit(edit); }}
            disabled={saving}
          />
        )}
        {saving && <Loader size={12} className="spin" />}
      </div>
    </div>
  );
}
