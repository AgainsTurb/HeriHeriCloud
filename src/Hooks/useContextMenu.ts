import { useState, useEffect } from 'react';

export function useContextMenu() {
  const [contextMenu, setContextMenu] = useState<{ x: number, y: number, targetId: number | null } | null>(null);

  const handleContextMenu = (e: React.MouseEvent, targetId: number | null = null) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, targetId });
  };

  const closeMenu = () => setContextMenu(null);

  useEffect(() => {
    if (contextMenu) {
      const handleClose = (e: Event) => {
        // Prevent closing if the user is clicking *inside* the context menu itself
        if ((e.target as HTMLElement).closest('.context-menu-box')) return;
        closeMenu();
      };
      
      // 'capture: true' intercepts the click before any other element can call stopPropagation()
      window.addEventListener('mousedown', handleClose, { capture: true });
      window.addEventListener('scroll', handleClose, { capture: true });
      
      return () => {
        window.removeEventListener('mousedown', handleClose, { capture: true });
        window.removeEventListener('scroll', handleClose, { capture: true });
      };
    }
  }, [contextMenu]);

  return { contextMenu, handleContextMenu, closeMenu };
}