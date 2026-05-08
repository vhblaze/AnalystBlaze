import { useRef, type ReactNode, type CSSProperties, type MouseEvent } from "react";
import { cn } from "@/lib/utils";

type Props = {
  children: ReactNode;
  className?: string;
  intensity?: number; // max degrees
  style?: CSSProperties;
};

/**
 * 3D holographic card: CSS transforms through refs, without React re-renders.
 */
export function TiltCard({ children, className, intensity = 8, style }: Props) {
  const innerRef = useRef<HTMLDivElement>(null);
  const glareRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<number | null>(null);
  const latestMoveRef = useRef<MouseEvent<HTMLDivElement> | null>(null);

  const handleMove = (e: MouseEvent<HTMLDivElement>) => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    latestMoveRef.current = e;
    if (frameRef.current !== null) return;
    frameRef.current = window.requestAnimationFrame(() => {
      const event = latestMoveRef.current;
      const el = innerRef.current;
      frameRef.current = null;
      if (!event || !el) return;
      const rect = el.getBoundingClientRect();
      const x = (event.clientX - rect.left) / rect.width;
      const y = (event.clientY - rect.top) / rect.height;
      const rx = (0.5 - y) * intensity * 2;
      const ry = (x - 0.5) * intensity * 2;
      el.style.transform = `rotateX(${rx.toFixed(2)}deg) rotateY(${ry.toFixed(2)}deg)`;
      if (glareRef.current) {
        glareRef.current.style.background = `radial-gradient(circle at ${x * 100}% ${y * 100}%, hsl(187 100% 60% / 0.18), transparent 55%)`;
      }
    });
  };

  const handleLeave = () => {
    if (frameRef.current !== null) {
      window.cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    }
    latestMoveRef.current = null;
    const el = innerRef.current;
    if (el) el.style.transform = "rotateX(0deg) rotateY(0deg)";
    if (glareRef.current) glareRef.current.style.background = "transparent";
  };

  return (
    <div
      className={cn("perspective-1000 group", className)}
      onMouseMove={handleMove}
      onMouseLeave={handleLeave}
      style={style}
    >
      <div
        ref={innerRef}
        className="preserve-3d glass-panel relative h-full w-full transition-transform duration-200 ease-out will-change-transform cyber-glow"
      >
        <div
          ref={glareRef}
          aria-hidden
          className="pointer-events-none absolute inset-0 rounded-2xl transition-opacity duration-200"
        />
        <div className="relative h-full w-full">{children}</div>
      </div>
    </div>
  );
}
