export interface Repository {
    id: string;
    name: string;
    files: FileInfo[];
}

export interface FileInfo {
    path: string;
    size: number;
    language: string;
}

export type AnalysisResult = {
    repo: Repository;
    symbols: SymbolInfo[];
    errors: string[];
};

export interface SymbolInfo {
    name: string;
    kind: 'Function' | 'Class' | 'Interface' | 'Variable';
    location: { file: string; line: number };
    references: string[];
}

export interface AnalysisOptions {
    maxDepth?: number;
    excludePatterns?: string[];
    languages?: string[];
}
