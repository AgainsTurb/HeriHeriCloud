import { useState, useEffect } from 'react';

export function useRectangleSelect(
  containerRef: React.RefObject<HTMLDivElement | null>,
  nodes: any[],
  setSelectedNodes: React.Dispatch<React.SetStateAction<Set<number>>>
) {
  const [selectionBox, setSelectionBox] = useState<{ startX: number, startY: number, currX: number, currY: number } | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let isSelecting = false;
    let hasDragged = false; // Tracks if the mouse actually moved
    let startX = 0, startY = 0;
    let baseSelection = new Set<number>(); // Locks in the selection state BEFORE the drag begins

    const handleMouseDown = (e: MouseEvent) => {
      // Only start rectangle select if clicking the empty space, NOT a file row
      if ((e.target as HTMLElement).closest('.file-row')) return;
      if (e.button !== 0) return; // Only left click

      const rect = container.getBoundingClientRect();
      startX = e.clientX - rect.left + container.scrollLeft;
      startY = e.clientY - rect.top + container.scrollTop;
      
      isSelecting = true;
      hasDragged = false;
      setSelectionBox({ startX, startY, currX: startX, currY: startY });
      
      // Capture the baseline selection so we can add/remove from it cleanly
      setSelectedNodes(prev => {
        baseSelection = (!e.ctrlKey && !e.metaKey) ? new Set() : new Set(prev);
        return baseSelection;
      });
    };

    const handleMouseMove = (e: MouseEvent) => {
      if (!isSelecting) return;
      hasDragged = true; // The user is actively drawing the box

      const rect = container.getBoundingClientRect();
      const currX = Math.max(0, Math.min(e.clientX - rect.left + container.scrollLeft, container.scrollWidth));
      const currY = Math.max(0, Math.min(e.clientY - rect.top + container.scrollTop, container.scrollHeight));
      
      setSelectionBox({ startX, startY, currX, currY });

      const minX = Math.min(startX, currX);
      const maxX = Math.max(startX, currX);
      const minY = Math.min(startY, currY);
      const maxY = Math.max(startY, currY);

      const rowElements = container.querySelectorAll('.file-row');
      const newSelection = new Set<number>();
      
      rowElements.forEach((el, index) => {
        const elRect = (el as HTMLElement).getBoundingClientRect();
        const elTop = elRect.top - rect.top + container.scrollTop;
        const elBottom = elRect.bottom - rect.top + container.scrollTop;
        const elLeft = elRect.left - rect.left + container.scrollLeft;
        const elRight = elRect.right - rect.left + container.scrollLeft;

        // Intersection Math
        const isIntersecting = !(maxX < elLeft || minX > elRight || maxY < elTop || minY > elBottom);
        if (isIntersecting && nodes[index]) {
          newSelection.add(nodes[index].id);
        }
      });
      
      // MINIMUM FIX B: Combine the new intersection with the BASE selection, not 'prev'
      setSelectedNodes(() => {
         const combined = new Set(baseSelection);
         newSelection.forEach(id => combined.add(id));
         return combined;
      });
    };

    const handleMouseUp = () => {
      isSelecting = false;
      setSelectionBox(null);
    };

    // MINIMUM FIX A: Intercept the native click event that happens right after MouseUp
    const handleClickCapture = (e: MouseEvent) => {
      if (hasDragged) {
        e.stopPropagation(); // Stops the click from reaching Home.tsx and triggering clearSelection()
        hasDragged = false;
      }
    };

    container.addEventListener('mousedown', handleMouseDown);
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    
    // Use the capture phase so this runs BEFORE React's synthetic event system
    container.addEventListener('click', handleClickCapture, { capture: true });

    return () => {
      container.removeEventListener('mousedown', handleMouseDown);
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
      container.removeEventListener('click', handleClickCapture, { capture: true });
    };
  }, [containerRef, nodes, setSelectedNodes]);

  return selectionBox;
}