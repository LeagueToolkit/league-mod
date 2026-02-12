import {
  closestCenter,
  DndContext,
  type DragEndEvent,
  DragOverlay,
  type DragStartEvent,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { useState } from "react";

import type { WorkshopLayer } from "@/lib/tauri";

import { LayerCard } from "./LayerCard";
import { SortableLayerCard } from "./SortableLayerCard";

interface SortableLayerListProps {
  layers: WorkshopLayer[];
  onReorder: (layerNames: string[]) => void;
  onEdit: (layer: WorkshopLayer) => void;
  onDelete: (layer: WorkshopLayer) => void;
}

export function SortableLayerList({ layers, onReorder, onEdit, onDelete }: SortableLayerListProps) {
  const [activeId, setActiveId] = useState<string | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const items = layers.map((l) => l.name);
  const activeDragLayer = activeId ? layers.find((l) => l.name === activeId) : null;

  function handleDragStart(event: DragStartEvent) {
    setActiveId(event.active.id as string);
  }

  function handleDragEnd(event: DragEndEvent) {
    setActiveId(null);
    const { active, over } = event;
    if (!over || active.id === over.id) return;

    const oldIndex = items.indexOf(active.id as string);
    const newIndex = items.indexOf(over.id as string);
    onReorder(arrayMove(items, oldIndex, newIndex));
  }

  function handleDragCancel() {
    setActiveId(null);
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onDragCancel={handleDragCancel}
    >
      <SortableContext items={items} strategy={verticalListSortingStrategy}>
        <div className="space-y-2">
          {layers.map((layer) => (
            <SortableLayerCard
              key={layer.name}
              layer={layer}
              onEdit={() => onEdit(layer)}
              onDelete={() => onDelete(layer)}
            />
          ))}
        </div>
      </SortableContext>
      <DragOverlay>
        {activeDragLayer && (
          <div className="opacity-90">
            <LayerCard layer={activeDragLayer} />
          </div>
        )}
      </DragOverlay>
    </DndContext>
  );
}
