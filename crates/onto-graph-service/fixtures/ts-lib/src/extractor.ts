import { FileInfo, SymbolInfo } from './types';

export class SymbolExtractor {
    async extract(file: FileInfo): Promise<SymbolInfo[]> {
        if (file.language === 'typescript' || file.language === 'javascript') {
            return this.extractFromSource(file);
        }
        return [];
    }

    private async extractFromSource(file: FileInfo): Promise<SymbolInfo[]> {
        return [{
            name: 'testSymbol',
            kind: 'Function',
            location: { file: file.path, line: 1 },
            references: [],
        }];
    }
}
