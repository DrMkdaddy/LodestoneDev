import { TaskPID } from "../../../deno_bindings/TaskPID.ts";


declare const __macro_pid: TaskPID;
declare const __instance_uuid: string | null;

// deno-lint-ignore no-explicit-any
declare const Deno: any;
const core = Deno[Deno.internal].core;

export function taskPid(): TaskPID {
    return __macro_pid;
}

export function instanceUUID(): string | null {
    return __instance_uuid;
}

export function lodestoneVersion(): string {
    return core.get_lodestone_version();
}