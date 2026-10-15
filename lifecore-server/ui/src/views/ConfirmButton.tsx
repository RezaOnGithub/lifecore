import { type ReactNode, useEffect, useState } from "react";

// A destructive button that asks first: the first click turns it into
// "Confirm …?" for a few seconds, and only a second click acts.
export function ConfirmButton({
  children,
  confirm,
  disabled = false,
  title,
  onConfirm,
}: {
  children: ReactNode;
  confirm: string;
  disabled?: boolean;
  title?: string | undefined;
  onConfirm: () => void;
}) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) {
      return;
    }
    const timer = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(timer);
  }, [armed]);
  return (
    <button
      type="button"
      className={armed ? "confirm" : undefined}
      disabled={disabled}
      title={title}
      onClick={() => {
        if (armed) {
          setArmed(false);
          onConfirm();
        } else {
          setArmed(true);
        }
      }}
    >
      {armed ? confirm : children}
    </button>
  );
}
