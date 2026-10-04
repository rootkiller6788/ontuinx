import { Repository, FileInfo, AnalysisResult, SymbolInfo, AnalysisOptions } from './types';
import { FileScanner } from './scanner';
import { SymbolExtractor } from './extractor';

export class RepositoryAnalyzer implements AnalysisOptions {
    private scanner: FileScanner;
    private extractor: SymbolExtractor;
    public maxDepth: number;
    public excludePatterns: string[];
    public languages: string[];

    constructor(options: AnalysisOptions = {}) {
        this.maxDepth = options.maxDepth ?? 10;
        this.excludePatterns = options.excludePatterns ?? ['node_modules', '.git'];
        this.languages = options.languages ?? ['*'];
        this.scanner = new FileScanner(this.maxDepth, this.excludePatterns);
        this.extractor = new SymbolExtractor();
    }

    async analyze(repo: Repository): Promise<AnalysisResult> {
        const files: FileInfo[] = this.scanner.scanRepo(repo);
        const symbols: SymbolInfo[] = [];
        const errors: string[] = [];

        for (const file of files) {
            if (!this.shouldAnalyze(file)) continue;
            try {
                const fileSymbols = await this.extractor.extract(file);
                symbols.push(...fileSymbols);
            } catch (e) {
                errors.push(`Failed to analyze ${file.path}: ${e}`);
            }
        }

        return { repo, symbols, errors };
    }

    private shouldAnalyze(file: FileInfo): boolean {
        if (this.languages.includes('*')) return true;
        return this.languages.includes(file.language);
    }
}

export class FileScanner {
    constructor(private maxDepth: number, private exclude: string[]) {}
    scanRepo(repo: Repository): FileInfo[] {
        return repo.files.filter(f => this.shouldInclude(f));
    }
    private shouldInclude(f: FileInfo): boolean {
        return !this.exclude.some(p => f.path.includes(p));
    }
}
