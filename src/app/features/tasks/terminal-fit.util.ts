type FitDimensions = { cols: number; rows: number };

export class TerminalFitManager {
    private fitScheduled = false;

    constructor(
        private readonly proposeDimensions: () => FitDimensions | undefined,
        private readonly fit: () => void,
        private readonly isSuspended: () => boolean,
    ) {}

    scheduleFit(): void {
        if (this.isSuspended()) {
            return;
        }
        this.scheduleFitAttempt(0);
        if (typeof document !== "undefined" && "fonts" in document) {
            void (
                document as Document & { fonts: FontFaceSet }
            ).fonts.ready.then(() => {
                if (!this.isSuspended()) {
                    this.scheduleFitAttempt(0);
                }
            });
        }
    }

    scheduleFitOnResize(): void {
        if (this.isSuspended() || this.fitScheduled) {
            return;
        }
        this.fitScheduled = true;
        requestAnimationFrame(() => {
            this.fitScheduled = false;
            if (!this.isSuspended()) {
                this.fit();
            }
        });
    }

    private scheduleFitAttempt(attempt: number): void {
        const maxAttempts = 30;
        requestAnimationFrame(() => {
            if (this.isSuspended()) {
                return;
            }
            const dimensions = this.proposeDimensions();
            if (dimensions?.cols && dimensions.rows) {
                this.fit();
                return;
            }
            if (attempt < maxAttempts) {
                this.scheduleFitAttempt(attempt + 1);
            }
        });
    }
}
