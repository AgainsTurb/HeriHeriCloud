import { useState, useEffect, MouseEvent as ReactMouseEvent } from "react";

export function useFileSelection(nodes: any[], onBatchDelete: () => void, onCut: (ids: number[]) => void, onPaste: () => void) {
  const [selectedNodes, setSelectedNodes] = useState<Set<number>>(new Set());
  const [lastSelected, setLastSelected] = useState<number | null>(null);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Ignore shortcuts if the user is typing inside an input field (like a modal)
      if (['INPUT', 'TEXTAREA'].includes((e.target as HTMLElement).tagName)) return;

      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
        e.preventDefault();
        setSelectedNodes(new Set(nodes.map(n => n.id)));
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'x') {
        if (selectedNodes.size > 0) onCut(Array.from(selectedNodes));
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'v') {
        onPaste();
      } else if (e.key === 'Delete') {
        if (selectedNodes.size > 0) onBatchDelete();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [nodes, selectedNodes, onBatchDelete, onCut, onPaste]);

  const handleRowClick = (e: ReactMouseEvent, index: number, id: number) => {
    e.stopPropagation();
    const newSet = new Set(selectedNodes);

    if (e.shiftKey && lastSelected !== null) {
      const start = Math.min(lastSelected, index);
      const end = Math.max(lastSelected, index);
      for (let i = start; i <= end; i++) newSet.add(nodes[i].id);
    } else if (e.ctrlKey || e.metaKey) {
      if (newSet.has(id)) newSet.delete(id);
      else newSet.add(id);
      setLastSelected(index);
    } else {
      newSet.clear();
      newSet.add(id);
      setLastSelected(index);
    }
    setSelectedNodes(newSet);
  };

  const clearSelection = () => {
    setSelectedNodes(new Set());
    setLastSelected(null);
  };

  return { selectedNodes, setSelectedNodes, handleRowClick, clearSelection };
}