import { bootstrapApplication } from "@angular/platform-browser";
import { appConfig } from "./app/features/shell/app.config";
import { AppComponent } from "./app/features/shell/components/app/app.component";

bootstrapApplication(AppComponent, appConfig).catch((err) =>
    console.error(err),
);
