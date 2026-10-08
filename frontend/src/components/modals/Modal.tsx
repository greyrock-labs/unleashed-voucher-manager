"use client";

import { ReactNode, useEffect, RefObject } from "react";

type Props = {
  onClose: () => void;
  /** Extra classes for the content container */
  contentClassName?: string;
  ref?: RefObject<HTMLDivElement | null>;
  children: ReactNode;
};

export default function Modal({
  onClose,
  contentClassName = "",
  ref,
  children,
}: Props) {
  useEffect(() => {
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);

    return () => {
      document.body.style.overflow = prevOverflow;
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  const onBackdropClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) onClose();
  };

  return (
    <div
      className="fixed inset-0 bg-overlay flex-center z-8000"
      onClick={onBackdropClick}
    >
      <div
        className={`bg-surface border border-default flex flex-col max-h-9/10 max-w-lg overflow-hidden relative rounded-xl shadow-2xl w-full ${contentClassName}`}
        ref={ref}
      >
        <button
          onClick={onClose}
          className="absolute top-4 right-4 p-1 flex-center rounded-full text-secondary hover:text-primary hover-scale btn"
          aria-label="Close"
        >
          <svg
            viewBox="0 0 24 24"
            className="w-8 h-8"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
        <div className="overflow-y-auto mr-3 mt-8 mb-2 p-6">{children}</div>
      </div>
    </div>
  );
}
