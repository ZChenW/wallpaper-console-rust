import SinglePageShell from './shell/SinglePageShell.tsx';
import { ThumbnailStoreProvider } from './state/ThumbnailStoreContext.tsx';

export default function App() {
  return (
    <ThumbnailStoreProvider>
      <SinglePageShell />
    </ThumbnailStoreProvider>
  );
}
