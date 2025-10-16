import { useEffect, useState } from "react";

interface FloatingMessageProps {
  x: number;
  y: number;
  message: string;
  onComplete: () => void;
}

export function FloatingMessage({ x, y, message, onComplete }: FloatingMessageProps) {
  const [isVisible, setIsVisible] = useState(true);

  useEffect(() => {
    const timer = setTimeout(() => {
      setIsVisible(false);
      setTimeout(onComplete, 300);
    }, 1000);

    return () => clearTimeout(timer);
  }, [onComplete]);

  return (
    <div
      className="floating-message"
      style={{
        left: `${x}px`,
        top: `${y}px`,
        opacity: isVisible ? 1 : 0,
        transform: `translateY(-${isVisible ? 0 : 60}px)`,
      }}
    >
      {message}
    </div>
  );
}
