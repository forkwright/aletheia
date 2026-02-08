// Stub: wizard UI removed for headless gateway builds

export type WizardSelectOption<T = string> = {
  value: T;
  label: string;
  hint?: string;
};

export type WizardPrompter = {
  text: (opts: {
    message: string;
    placeholder?: string;
    initialValue?: string;
    validate?: (value: string) => string | undefined;
  }) => Promise<string>;
  select: <T = string>(opts: {
    message: string;
    options: WizardSelectOption<T>[];
    initialValue?: T;
  }) => Promise<T>;
  confirm: (opts: { message: string; initialValue?: boolean }) => Promise<boolean>;
  multiselect: <T = string>(opts: {
    message: string;
    options: WizardSelectOption<T>[];
    initialValues?: T[];
    required?: boolean;
  }) => Promise<T[]>;
  note: (message: string, title?: string) => Promise<void> | void;
  intro: (message: string) => Promise<void> | void;
  outro: (message: string) => Promise<void> | void;
  spinner: () => { start: (message?: string) => void; stop: (message?: string) => void };
  progress: (message: string) => {
    update: (message: string) => void;
    stop: (message?: string) => void;
  };
};

export class WizardCancelledError extends Error {
  constructor(message = "Wizard cancelled") {
    super(message);
    this.name = "WizardCancelledError";
  }
}
